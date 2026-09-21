//! The optional inference (Mimir) capability seam. Core never *requires* Mimir:
//! AI is an **optional capability discovered at runtime** (KDD-3). A function that
//! can use inference holds it as `Option<Box<dyn Inference>>` and defines a
//! deterministic fallback for when it is absent — so any product built on core
//! operates in a defined degraded mode with Mimir gone. [DD-6] [AC-M-9]

/// A local inference capability (implemented by the Mimir plane in a later phase).
/// `Send + Sync` so it can be shared across gateway threads.
pub trait Inference: Send + Sync {
    /// Suggest a value for `key`, or `None` if it has no suggestion.
    fn suggest(&self, key: &str) -> Option<String>;
}
