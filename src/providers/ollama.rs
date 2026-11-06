use super::openai_compat::OpenAiCompatProvider;

pub struct OllamaProvider;

impl OllamaProvider {
    /// Factory for creating an Ollama OpenAI-compatible provider.
    pub fn create(endpoint: String, model: String) -> OpenAiCompatProvider {
        OpenAiCompatProvider::ollama(endpoint, model)
    }

    /// Backwards-compatible constructor forwarding to `create`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(endpoint: String, model: String) -> OpenAiCompatProvider {
        Self::create(endpoint, model)
    }
}
