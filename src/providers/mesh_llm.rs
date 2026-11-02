use super::openai_compat::OpenAiCompatProvider;

pub struct MeshLlmProvider;

impl MeshLlmProvider {
    pub fn new(endpoint: String, model: String) -> OpenAiCompatProvider {
        OpenAiCompatProvider::mesh_llm(endpoint, model)
    }
}
