//! The delegated in-page assistant seam. The extension holds no model and does no
//! privileged work; generation is brokered to the enclosure over the authenticated
//! channel. Production bridges this to enclosure Mimir; tests use a deterministic
//! mock. `[FR-C-8]`

/// Generates an answer for the extension from the shared selection and enclosure
/// grounding only. This is the brokered inference call.
pub trait InPageAssistant {
    /// The error type the brokered call may return.
    type Error: std::fmt::Display;
    /// Answer `message` using only `shared_selection` and `grounding`.
    fn assist(
        &self,
        shared_selection: &str,
        grounding: &[String],
        message: &str,
    ) -> Result<String, Self::Error>;
}

/// A deterministic assistant for tests. It answers using only the inputs it is
/// given — proving the broker passes it nothing but the shared selection and
/// grounding.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockInPageAssistant;

impl InPageAssistant for MockInPageAssistant {
    type Error = std::convert::Infallible;
    fn assist(
        &self,
        shared_selection: &str,
        grounding: &[String],
        message: &str,
    ) -> Result<String, Self::Error> {
        Ok(format!(
            "re '{message}': based on the shared selection ({} chars) and {} grounded item(s)",
            shared_selection.len(),
            grounding.len()
        ))
    }
}
