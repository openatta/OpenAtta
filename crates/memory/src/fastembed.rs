//! FastEmbed-based local embedding provider
//!
//! Uses the `fastembed` crate for on-device text embedding.
//! Default model: AllMiniLML6V2 (384 dimensions).

use atta_types::AttaError;

use crate::embedding::EmbeddingProvider;

/// Local embedding provider backed by fastembed
pub struct FastEmbedProvider {
    model: fastembed::TextEmbedding,
    dims: usize,
}

impl FastEmbedProvider {
    /// Create a new FastEmbedProvider with the specified model and optional cache directory.
    ///
    /// If `cache_dir` is provided, model files are stored there instead of the
    /// default HuggingFace cache location.
    pub fn new(
        model_type: fastembed::EmbeddingModel,
        cache_dir: Option<std::path::PathBuf>,
    ) -> Result<Self, AttaError> {
        let dims = match model_type {
            fastembed::EmbeddingModel::AllMiniLML6V2 => 384,
            fastembed::EmbeddingModel::AllMiniLML12V2 => 384,
            fastembed::EmbeddingModel::BGESmallENV15 => 384,
            fastembed::EmbeddingModel::BGEBaseENV15 => 768,
            fastembed::EmbeddingModel::BGELargeENV15 => 1024,
            _ => 384, // safe default
        };

        let mut opts =
            fastembed::InitOptions::new(model_type).with_show_download_progress(true);
        if let Some(dir) = cache_dir {
            opts = opts.with_cache_dir(dir);
        }
        let model = fastembed::TextEmbedding::try_new(opts).map_err(AttaError::Other)?;

        Ok(Self { model, dims })
    }

    /// Create with default model (AllMiniLML6V2, 384 dims)
    pub fn default_model() -> Result<Self, AttaError> {
        Self::new(fastembed::EmbeddingModel::AllMiniLML6V2, None)
    }

    /// Create with default model and a custom cache directory for model files.
    pub fn default_model_with_cache(cache_dir: std::path::PathBuf) -> Result<Self, AttaError> {
        Self::new(fastembed::EmbeddingModel::AllMiniLML6V2, Some(cache_dir))
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for FastEmbedProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AttaError> {
        let texts = vec![text.to_string()];
        // fastembed is CPU-bound; run in blocking thread pool
        let result = self.model.embed(texts, None).map_err(AttaError::Other)?;
        result
            .into_iter()
            .next()
            .ok_or_else(|| AttaError::Other(anyhow::anyhow!("empty embedding result")))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AttaError> {
        let texts: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        self.model.embed(texts, None).map_err(AttaError::Other)
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::cosine_similarity;
    use std::sync::OnceLock;

    /// Shared model singleton — loaded once, reused by all tests.
    /// Eliminates HF cache lock contention when tests run in parallel.
    fn shared_provider() -> &'static FastEmbedProvider {
        static PROVIDER: OnceLock<FastEmbedProvider> = OnceLock::new();
        PROVIDER.get_or_init(|| {
            FastEmbedProvider::default_model().expect("failed to load fastembed model")
        })
    }

    #[tokio::test]
    async fn test_fastembed_loads_model() {
        let provider = shared_provider();
        assert_eq!(provider.dimensions(), 384);
    }

    #[tokio::test]
    async fn test_fastembed_embed_dimensions() {
        let provider = shared_provider();
        let embedding = provider.embed("hello world").await.unwrap();
        assert_eq!(embedding.len(), 384);
        assert!(embedding.iter().any(|&v| v != 0.0));
    }

    #[tokio::test]
    async fn test_fastembed_semantic_similarity() {
        let provider = shared_provider();
        let cat = provider.embed("cat").await.unwrap();
        let dog = provider.embed("dog").await.unwrap();
        let database = provider.embed("relational database management").await.unwrap();

        let cat_dog = cosine_similarity(&cat, &dog);
        let cat_db = cosine_similarity(&cat, &database);
        assert!(
            cat_dog > cat_db,
            "expected cat-dog ({cat_dog}) > cat-database ({cat_db})"
        );
    }

    #[tokio::test]
    async fn test_fastembed_batch() {
        let provider = shared_provider();
        let texts = &["hello", "world"];
        let batch = provider.embed_batch(texts).await.unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].len(), 384);
        assert_eq!(batch[1].len(), 384);

        let single_hello = provider.embed("hello").await.unwrap();
        let sim = cosine_similarity(&batch[0], &single_hello);
        assert!(
            (sim - 1.0).abs() < 1e-5,
            "batch and single embed differ: similarity = {sim}"
        );
    }
}
