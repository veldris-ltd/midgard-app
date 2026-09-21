//! The concrete serving engines, all behind [`crate::backend::ModelBackend`].
//! [FR-MI-1] [DD-4]
//!
//! - [`LocalReference`] — the deterministic in-enclosure reference engine that
//!   proves the acceptance criteria offline with no GPU.
//! - [`SidecarBackend`] — a model served by an OpenAI-compatible sidecar on loopback:
//!   llama.cpp on a CPU node, vLLM on a GPU node.
//! - [`GpuDockerBackend`] — the vLLM engine with its hardware profile and the hardened
//!   container launch for a bare-metal GPU host, selected automatically on a GPU node.

mod gpu_docker;
mod local;
mod openai_compat;

pub use gpu_docker::{GpuDockerBackend, LaunchSpec};
pub use local::LocalReference;
pub use openai_compat::{Engine, SidecarBackend};
