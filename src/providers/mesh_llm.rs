use super::openai_compat::OpenAiCompatProvider;

pub struct MeshLlmProvider;

impl MeshLlmProvider {
    /// Factory for creating a Mesh-LLM OpenAI-compatible provider.
    pub fn create(endpoint: String, model: String) -> OpenAiCompatProvider {
        OpenAiCompatProvider::mesh_llm(endpoint, model)
    }

    /// Backwards-compatible constructor forwarding to `create`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(endpoint: String, model: String) -> OpenAiCompatProvider {
        Self::create(endpoint, model)
    }
}
