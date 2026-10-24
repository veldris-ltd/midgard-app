//! The GPU serving engine: vLLM in a container, co-located with the model plane.
//! [FR-MI-1]
//!
//! It advertises a concrete [`HwProfile`], so the autonomous selector places it only
//! where the node has the GPUs to serve it, and degrades to a smaller tier or the CPU
//! engines where it does not. It serves through [`SidecarBackend`]: vLLM's
//! OpenAI-compatible streaming API on **loopback**, never the inter-service data plane.
//!
//! On a bare-metal GPU host it also describes, and can start, the exact hardened
//! `docker run` for its sidecar ([`LaunchSpec`]): a digest-pinned image, a read-only
//! root filesystem and read-only weights, no capabilities, published on loopback only.
//! In a pod, the Helm chart runs the same image as a sidecar container instead. The
//! <800 ms first-token SLO (NFR-PERF-1) is validated on target GPU hardware in the
//! deployment-tier suite (RT-40).

use std::net::SocketAddr;

use crate::backend::{BackendDescriptor, InferError, ModelBackend, PreparedRequest, RawCompletion};
use crate::backends::openai_compat::{Engine, SidecarBackend};
use crate::hardware::HwProfile;
use crate::quant::Quant;

/// The exact command that launches the co-located GPU serving sidecar. Returned by
/// [`GpuDockerBackend::launch_spec`] so it can be reviewed, diffed, and tested without
/// running Docker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    /// The program to exec (the container runtime).
    pub program: String,
    /// Its arguments, in order.
    pub args: Vec<String>,
}

/// A GPU serving engine backed by a vLLM container.
pub struct GpuDockerBackend {
    sidecar: SidecarBackend,
    /// The serving image, pinned by digest.
    image: String,
    /// The host directory holding the verified weights, mounted read-only.
    weights_dir: String,
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

impl GpuDockerBackend {
    /// A vLLM engine requiring `hw` (optionally multi-node), serving the weights in
    /// `weights_dir` under its id. `rank` orders it against other GPU tiers.
    pub fn vllm(
        id: impl Into<String>,
        image: impl Into<String>,
        weights_dir: impl Into<String>,
        hw: HwProfile,
        rank: u8,
    ) -> Self {
        let id = id.into();
        let families = ["Mistral", "Nemotron", "Gemma", "Granite"]
            .map(String::from)
            .to_vec();
        Self {
            sidecar: SidecarBackend::vllm(id.clone(), loopback(8_000), id, families, hw, rank)
                .expect("a loopback sidecar address is always accepted"),
            image: image.into(),
            weights_dir: weights_dir.into(),
        }
    }

    /// The loopback port the sidecar listens on.
    pub fn port(&self) -> u16 {
        self.sidecar.addr().port()
    }

    /// Point this backend at a sidecar already listening on loopback `port`.
    pub fn with_port(mut self, port: u16) -> Self {
        let descriptor = self.sidecar.descriptor().clone();
        let model = descriptor.id.clone();
        self.sidecar = SidecarBackend::new(descriptor, Engine::Vllm, loopback(port), model)
            .expect("a loopback sidecar address is always accepted");
        self
    }

    /// The sidecar client this engine serves through.
    pub fn sidecar(&self) -> &SidecarBackend {
        &self.sidecar
    }

    /// Start the sidecar from the [`LaunchSpec`] on a bare-metal GPU host, returning
    /// the child. Refused unless the image is pinned by digest. Serving then goes
    /// through [`ModelBackend::infer_batch`] over loopback, and fails closed until the
    /// sidecar answers. [FR-MI-1]
    pub fn launch(&self, quant: Quant) -> std::io::Result<std::process::Child> {
        if !self.image.contains("@sha256:") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("the serving image {} is not pinned by digest", self.image),
            ));
        }
        let spec = self.launch_spec(quant);
        std::process::Command::new(&spec.program)
            .args(&spec.args)
            .spawn()
    }

    /// The hardened `docker run …` that launches the sidecar for a quantisation.
    pub fn launch_spec(&self, quant: Quant) -> LaunchSpec {
        let port = self.port().to_string();
        let mut args: Vec<String> = [
            "run",
            "--rm",
            "--gpus",
            "all",
            "--read-only",
            "--tmpfs",
            "/tmp",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges:true",
        ]
        .map(String::from)
        .to_vec();
        args.extend([
            // Published only on loopback: never reachable off-host.
            "-p".to_string(),
            format!("127.0.0.1:{port}:{port}"),
            // The verified weights, read-only.
            "-v".to_string(),
            format!("{}:/models:ro", self.weights_dir),
            self.image.clone(),
            "--model".to_string(),
            "/models".to_string(),
            "--served-model-name".to_string(),
            self.sidecar.descriptor().id.clone(),
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--port".to_string(),
            port,
        ]);
        if let Some(q) = quant.vllm_arg() {
            args.push("--quantization".to_string());
            args.push(q.to_string());
        }
        LaunchSpec {
            program: "docker".to_string(),
            args,
        }
    }
}

impl ModelBackend for GpuDockerBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        self.sidecar.descriptor()
    }

    fn infer_batch(&self, batch: &[PreparedRequest]) -> Result<Vec<RawCompletion>, InferError> {
        // Mimir never silently reaches for a hosted model: an unreachable sidecar fails
        // the call. [DD-4] [AC-MI-1]
        self.sidecar.infer_batch(batch)
    }
}
