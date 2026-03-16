//! Embedding provider trait and utilities

use atta_types::AttaError;

/// Trait for embedding text into vectors
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync + 'static {
    /// Embed text into a vector
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AttaError>;

    /// Batch-embed multiple texts (default: sequential calls to [`embed`])
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AttaError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// Dimensionality of the embedding vectors
    fn dimensions(&self) -> usize;
}

/// No-op embedding provider (FTS-only mode)
pub struct NoopEmbeddingProvider;

#[async_trait::async_trait]
impl EmbeddingProvider for NoopEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, AttaError> {
        Ok(Vec::new())
    }

    fn dimensions(&self) -> usize {
        0
    }
}

/// Compute cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (ai, bi) in a.iter().zip(b.iter()) {
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Serialize f32 embedding vector to bytes (little-endian)
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &v in embedding {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Deserialize bytes to f32 embedding vector (little-endian)
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_embedding_roundtrip() {
        let original = vec![1.0f32, 2.5, -3.15, 0.0];
        let bytes = embedding_to_bytes(&original);
        let restored = bytes_to_embedding(&bytes);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_empty_vectors() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }
}
