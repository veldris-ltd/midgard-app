//! Hardware capability model and node probe.
//!
//! The plane picks the best serving engine a node can actually run (§2.a
//! decision). A [`NodeProfile`] describes what the node has; each backend
//! declares the [`HwProfile`] it needs; the selector matches them. This is the
//! same eligibility machinery the Phase-3 Model Registry uses for artefacts
//! (`HwProfile`, SAD_02b §7, FR-MS-5) — built once, here, and reused.
//!
//! Phase-2 probing is deliberately dependency-free and **fails safe**: with no
//! way to prove a GPU is present, it reports none, so the plane degrades to the
//! CPU/reference engine rather than trying to place weights it cannot serve.
//! Values are overridable by operators/ops via environment variables, which also
//! makes selection deterministically testable. Real NVML/CUDA probing is a
//! GPU-node integration that plugs into this same struct. [NFR-PERF-1]

/// What a serving node physically offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeProfile {
    /// Number of GPUs visible to the node.
    pub gpus: u32,
    /// VRAM per GPU, in GiB (the smallest GPU, if heterogeneous).
    pub vram_gb_per_gpu: u32,
    /// Whether a CUDA/ROCm compute stack is available.
    pub accel: bool,
    /// Whether the node participates in a multi-node serving cluster.
    pub multinode: bool,
    /// Logical CPU cores.
    pub cpu_cores: u32,
    /// System RAM in GiB (0 = unknown).
    pub ram_gb: u32,
    /// Whether the process is running inside a container.
    pub in_container: bool,
}

impl NodeProfile {
    /// A CPU-only node with no accelerator — the safe default and the shape of
    /// the offline CI/test harness.
    pub fn cpu_only(cpu_cores: u32) -> Self {
        Self {
            gpus: 0,
            vram_gb_per_gpu: 0,
            accel: false,
            multinode: false,
            cpu_cores,
            ram_gb: 0,
            in_container: false,
        }
    }

    /// Build a profile from a value getter (e.g. `|k| std::env::var(k).ok()`).
    /// Recognised keys: `MIMIR_GPUS`, `MIMIR_VRAM_GB`, `MIMIR_ACCEL` (1/true),
    /// `MIMIR_MULTINODE` (1/true), `MIMIR_CPU_CORES`, `MIMIR_RAM_GB`,
    /// `MIMIR_IN_CONTAINER` (1/true). Missing/unparseable values fail safe.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Self {
        let u = |k: &str| -> u32 { get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(0) };
        let b = |k: &str| -> bool {
            matches!(
                get(k).map(|v| v.trim().to_ascii_lowercase()).as_deref(),
                Some("1") | Some("true") | Some("yes")
            )
        };
        let gpus = u("MIMIR_GPUS");
        // A visible GPU implies an accelerator stack unless one is explicitly
        // disabled; no GPU means no accelerator regardless.
        let accel = gpus > 0
            && !matches!(
                get("MIMIR_ACCEL")
                    .map(|v| v.trim().to_ascii_lowercase())
                    .as_deref(),
                Some("0") | Some("false") | Some("no")
            );
        Self {
            gpus,
            vram_gb_per_gpu: u("MIMIR_VRAM_GB"),
            accel,
            multinode: b("MIMIR_MULTINODE"),
            cpu_cores: {
                let c = u("MIMIR_CPU_CORES");
                if c > 0 {
                    c
                } else {
                    std::thread::available_parallelism()
                        .map(|n| n.get() as u32)
                        .unwrap_or(1)
                }
            },
            ram_gb: u("MIMIR_RAM_GB"),
            in_container: b("MIMIR_IN_CONTAINER") || std::path::Path::new("/.dockerenv").exists(),
        }
    }
}

/// Probe the running node. Reads environment overrides, else fails safe to
/// CPU-only. Never contacts the network. [NFR-PRV-1]
pub fn probe() -> NodeProfile {
    let mut p = NodeProfile::from_env(|k| std::env::var(k).ok());
    // If no GPU override was given, we cannot prove an accelerator exists without
    // a native probe, so we report CPU-only and let the plane degrade.
    if std::env::var("MIMIR_GPUS").is_err() {
        p.gpus = 0;
        p.vram_gb_per_gpu = 0;
        p.accel = false;
    }
    p
}

/// The hardware a backend needs before it may be placed on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwProfile {
    /// Minimum GPUs required (0 = runs on CPU).
    pub min_gpus: u32,
    /// Minimum VRAM per GPU, in GiB.
    pub min_vram_gb: u32,
    /// Requires a multi-node cluster.
    pub multinode: bool,
}

impl HwProfile {
    /// No special hardware — runs anywhere (the reference/CPU floor).
    pub fn none() -> Self {
        Self {
            min_gpus: 0,
            min_vram_gb: 0,
            multinode: false,
        }
    }
    /// A single-GPU requirement of `min_vram_gb`.
    pub fn single_gpu(min_vram_gb: u32) -> Self {
        Self {
            min_gpus: 1,
            min_vram_gb,
            multinode: false,
        }
    }
    /// A multi-GPU / multi-node requirement.
    pub fn multi(min_gpus: u32, min_vram_gb: u32, multinode: bool) -> Self {
        Self {
            min_gpus,
            min_vram_gb,
            multinode,
        }
    }

    /// Whether `node` can satisfy this profile.
    pub fn is_satisfied_by(&self, node: &NodeProfile) -> bool {
        if self.min_gpus == 0 {
            return true; // CPU-eligible everywhere
        }
        node.accel
            && node.gpus >= self.min_gpus
            && node.vram_gb_per_gpu >= self.min_vram_gb
            && (!self.multinode || node.multinode)
    }
}
