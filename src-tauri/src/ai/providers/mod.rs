//! Provider registry. `anthropic.rs` (Messages API over raw HTTP/SSE) and `openai_compat.rs`
//! (chat/completions + embeddings) are implemented in the AI-providers module.
use crate::ai::llm::{LlmProvider, ResolvedModel};
use crate::error::{CaretError, Result};
use std::sync::Arc;

pub mod sse;
pub mod anthropic;
pub mod openai_compat;

pub fn build(resolved: &ResolvedModel) -> Result<Arc<dyn LlmProvider>> {
    use crate::core::settings::ProviderKind as K;
    match resolved.provider.kind {
        K::Anthropic | K::AnthropicCompatible => Ok(Arc::new(anthropic::AnthropicProvider::new(resolved)?)),
        K::Openai | K::OpenaiCompatible | K::AzureOpenai | K::Ollama | K::Openrouter | K::Gemini => Ok(Arc::new(openai_compat::OpenAiCompatProvider::new(resolved)?)),
    }
}

pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("Caret/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .pool_max_idle_per_host(4)
        .build()
        .map_err(|e| CaretError::internal(e.to_string()))
}
