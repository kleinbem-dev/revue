use super::openai_compat::OpenAiCompatProvider;

pub struct OllamaProvider;

impl OllamaProvider {
    pub fn new(endpoint: String, model: String) -> OpenAiCompatProvider {
        OpenAiCompatProvider::ollama(endpoint, model)
    }
}
