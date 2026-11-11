#![doc = "Planned for Iteration 10"]

/// A request to a language model.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub prompt: String,
    pub max_tokens: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("no LLM provider is configured")]
    NotConfigured,
    #[error("provider error: {0}")]
    Provider(String),
}

pub trait LlmProvider: Send + Sync {
    fn complete(&self, req: &LlmRequest) -> Result<String, LlmError>;
}
