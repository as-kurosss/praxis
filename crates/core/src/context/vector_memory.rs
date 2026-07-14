//! **Vector Memory** — in-memory vector store with cosine similarity search,
//! plus a hybrid search that combines FTS5 keyword matching with semantic
//! vector similarity.

/// Callback that takes a text string and returns its embedding vector.
pub type EmbeddingFn = std::sync::Arc<dyn Fn(&str) -> Vec<f64> + Send + Sync>;

/// A record in the in-memory vector store.
#[derive(Debug, Clone)]
pub struct VectorRecord {
    /// Unique identifier for this record.
    pub id: String,
    /// The embedding vector.
    pub embedding: Vec<f64>,
    /// Session ID this record belongs to.
    pub session_id: String,
    /// Text content this embedding represents.
    pub text: String,
    /// Additional metadata (JSON).
    pub metadata: serde_json::Value,
    /// Unix timestamp when this record was created.
    pub timestamp: i64,
}

/// In-memory vector store with cosine similarity search.
#[derive(Debug, Clone)]
pub struct VectorMemory {
    records: Vec<VectorRecord>,
}

impl VectorMemory {
    /// Create a new empty vector store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Add a record to the store.
    pub fn add(&mut self, record: VectorRecord) {
        self.records.push(record);
    }

    /// Remove a record by ID.
    /// Returns     rue if the record was found and removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let len = self.records.len();
        self.records.retain(|r| r.id != id);
        self.records.len() < len
    }

    /// Search the store for records most similar to query_embedding.
    /// Returns up to     op_k results ordered by descending similarity.
    #[must_use]
    pub fn search(&self, query_embedding: &[f64], top_k: usize) -> Vec<ScoredItem> {
        if self.records.is_empty() || query_embedding.is_empty() {
            return Vec::new();
        }

        let query_norm = norm(query_embedding);
        if query_norm == 0.0 {
            return Vec::new();
        }

        let mut scored: Vec<ScoredItem> = self
            .records
            .iter()
            .map(|r| {
                let sim = cosine_similarity(query_embedding, &r.embedding, query_norm);
                ScoredItem {
                    id: r.id.clone(),
                    session_id: r.session_id.clone(),
                    text: r.text.clone(),
                    metadata: r.metadata.clone(),
                    timestamp: r.timestamp,
                    score: sim,
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
        scored
    }

    /// Return the number of records in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns     rue if the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Clear all records.
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

impl Default for VectorMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// A scored search result from the vector store.
#[derive(Debug, Clone)]
pub struct ScoredItem {
    /// Record ID.
    pub id: String,
    /// Session ID.
    pub session_id: String,
    /// Text content.
    pub text: String,
    /// Additional metadata.
    pub metadata: serde_json::Value,
    /// Timestamp.
    pub timestamp: i64,
    /// Cosine similarity score (0.0 – 1.0).
    pub score: f64,
}

/// Compute the L2 norm of a vector.
fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// Compute cosine similarity between two vectors.
/// query_norm is the pre-computed norm of the query vector.
fn cosine_similarity(a: &[f64], b: &[f64], query_norm: f64) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let b_norm = norm(b);
    if b_norm == 0.0 {
        return 0.0;
    }
    dot / (query_norm * b_norm)
}

/// Combines FTS5 keyword search results with vector similarity results
/// using reciprocal rank fusion (RRF) or simple score interpolation.
#[derive(Debug, Clone)]
pub struct HybridSearch {
    /// Weight for the keyword search score (0.0 – 1.0).
    pub keyword_weight: f64,
    /// Weight for the vector search score (0.0 – 1.0).
    pub vector_weight: f64,
}

impl HybridSearch {
    /// Create a new hybrid search with equal weights (0.5 each).
    #[must_use]
    pub fn new() -> Self {
        Self {
            keyword_weight: 0.5,
            vector_weight: 0.5,
        }
    }

    /// Create a hybrid search with custom weights.
    #[must_use]
    pub fn with_weights(keyword_weight: f64, vector_weight: f64) -> Self {
        Self {
            keyword_weight,
            vector_weight,
        }
    }

    /// Fuse keyword search results (from FTS5) with vector search results.
    ///
    /// * keyword_results — pairs of (rank, id) from FTS5, where rank 0 = best.
    /// * ector_results — scored items from vector search.
    /// * `op_k` - maximum number of results to return.
    ///
    /// Returns fused results sorted by combined score.
    #[must_use]
    pub fn fuse(
        &self,
        keyword_results: &[(usize, String)],
        vector_results: &[ScoredItem],
        top_k: usize,
    ) -> Vec<FusedItem> {
        use std::collections::HashMap;

        // RRF score for keyword results
        let mut scores: HashMap<String, FusedItem> = HashMap::new();

        for (rank, id) in keyword_results {
            let rrf_score = 1.0 / (60.0 + *rank as f64);
            let entry = scores.entry(id.clone()).or_insert(FusedItem {
                id: id.clone(),
                session_id: String::new(),
                text: String::new(),
                metadata: serde_json::Value::Null,
                timestamp: 0,
                combined_score: 0.0,
                keyword_score: rrf_score * self.keyword_weight,
                vector_score: 0.0,
            });
            entry.combined_score += rrf_score * self.keyword_weight;
        }

        // Add vector scores
        for item in vector_results {
            let entry = scores.entry(item.id.clone()).or_insert(FusedItem {
                id: item.id.clone(),
                session_id: item.session_id.clone(),
                text: item.text.clone(),
                metadata: item.metadata.clone(),
                timestamp: item.timestamp,
                combined_score: 0.0,
                keyword_score: 0.0,
                vector_score: item.score * self.vector_weight,
            });
            entry.vector_score = item.score * self.vector_weight;
            entry.combined_score += item.score * self.vector_weight;
            // Fill in metadata if not already present
            if entry.text.is_empty() {
                entry.text = item.text.clone();
                entry.session_id = item.session_id.clone();
                entry.metadata = item.metadata.clone();
                entry.timestamp = item.timestamp;
            }
        }

        let mut fused: Vec<FusedItem> = scores.into_values().collect();
        fused.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        fused.truncate(top_k);
        fused
    }
}

impl Default for HybridSearch {
    fn default() -> Self {
        Self::new()
    }
}

/// A fused result from hybrid keyword + vector search.
#[derive(Debug, Clone)]
pub struct FusedItem {
    /// Record ID.
    pub id: String,
    /// Session ID.
    pub session_id: String,
    /// Text content.
    pub text: String,
    /// Additional metadata.
    pub metadata: serde_json::Value,
    /// Timestamp.
    pub timestamp: i64,
    /// Combined score from keyword and vector.
    pub combined_score: f64,
    /// The keyword search contribution.
    pub keyword_score: f64,
    /// The vector search contribution.
    pub vector_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(id: &str, embedding: Vec<f64>, text: &str) -> VectorRecord {
        VectorRecord {
            id: id.to_string(),
            embedding,
            session_id: "test-session".into(),
            text: text.to_string(),
            metadata: serde_json::json!({}),
            timestamp: 1_000_000,
        }
    }

    #[test]
    fn test_empty_store() {
        let store = VectorMemory::new();
        assert!(store.is_empty());
        let results = store.search(&[1.0, 0.0], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v, norm(&v));
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b, norm(&a));
        assert!((sim - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_vector_search_returns_nearest() {
        let mut store = VectorMemory::new();
        store.add(make_record("a", vec![1.0, 0.0], "rust programming"));
        store.add(make_record("b", vec![0.0, 1.0], "cooking recipes"));

        let results = store.search(&[1.0, 0.1], 5);
        assert_eq!(results[0].id, "a");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_hybrid_fuse() {
        let hybrid = HybridSearch::new();
        let keyword = vec![(0usize, "a".into()), (1usize, "b".into())];
        let vector = vec![
            ScoredItem {
                id: "b".into(),
                session_id: "s".into(),
                text: "text b".into(),
                metadata: serde_json::json!({}),
                timestamp: 0,
                score: 0.9,
            },
            ScoredItem {
                id: "c".into(),
                session_id: "s".into(),
                text: "text c".into(),
                metadata: serde_json::json!({}),
                timestamp: 0,
                score: 0.8,
            },
        ];

        let results = hybrid.fuse(&keyword, &vector, 5);
        assert_eq!(results.len(), 3);
        // b has both keyword and high vector score, should rank first
        assert_eq!(results[0].id, "b");
    }
}
