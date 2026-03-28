use anyhow::Result;
use std::sync::RwLock;
use tracing::info;

/// In-memory vector index for fast cosine similarity search.
pub struct VectorIndex {
    inner: RwLock<IndexData>,
}

struct IndexData {
    /// (photo_id, normalized_vector)
    entries: Vec<(i64, Vec<f32>)>,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub photo_id: i64,
    pub score: f32,
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(IndexData {
                entries: Vec::new(),
            }),
        }
    }

    /// Load all embeddings from database into memory.
    pub async fn load_from_db(&self, pool: &sqlx::SqlitePool) -> Result<()> {
        use photomind_storage::repo::embeddings::EmbeddingRepo;

        let all = EmbeddingRepo::load_all(pool).await?;
        let mut data = self.inner.write().unwrap();
        data.entries = all
            .into_iter()
            .map(|(id, vec)| {
                let norm = normalize(&vec);
                (id, norm)
            })
            .collect();

        info!("Loaded {} vectors into index", data.entries.len());
        Ok(())
    }

    /// Add a single embedding to the index.
    pub fn add(&self, photo_id: i64, vector: Vec<f32>) {
        let norm = normalize(&vector);
        let mut data = self.inner.write().unwrap();
        // Remove existing entry for this photo if any
        data.entries.retain(|(id, _)| *id != photo_id);
        data.entries.push((photo_id, norm));
    }

    /// Search for the top-k most similar vectors to the query.
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<SearchHit> {
        let query_norm = normalize(query);
        let data = self.inner.read().unwrap();

        let mut scores: Vec<SearchHit> = data
            .entries
            .iter()
            .map(|(id, vec)| SearchHit {
                photo_id: *id,
                score: cosine_similarity(&query_norm, vec),
            })
            .collect();

        // Sort by descending score
        scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        scores
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().entries.len()
    }
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / mag).collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    // Both vectors are already normalized, so dot product = cosine similarity
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
