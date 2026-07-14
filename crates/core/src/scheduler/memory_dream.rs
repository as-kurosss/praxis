//! **MemoryDream** — periodic memory consolidation and deduplication.
//!
//! A scheduled [`Loop`] that scans episodic memory for duplicate or highly
//! similar entries, consolidates them, and optionally reorganizes memory
//! around topics.  Running this periodically keeps the memory store lean
//! and maximally informative.
//!
//! # Consolidation strategy
//!
//! 1. **Keyword overlap** — entries whose keyword sets overlap by more than
//!    `similarity_threshold` (default 80 %) are candidates for merging.
//! 2. **Recency wins** — when merging, the more recent entry is kept and the
//!    older one is removed.
//! 3. **Reorganisation** — after deduplication, entries are grouped by shared
//!    topic keywords and a consolidated summary is produced for each group.
//!
//! # Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use std::sync::Mutex;
//! use praxis_core::memory::EpisodicMemory;
//! use praxis_core::scheduler::{Schedule, ScheduledTask, MemoryDreamLoop};
//!
//! let memory = Arc::new(Mutex::new(EpisodicMemory::new()));
//!
//! let dream_loop = MemoryDreamLoop::new(
//!     memory,
//!     "Consolidate and summarise the following conversation history.",
//! );
//!
//! let task = ScheduledTask::new(
//!     "memory-dream",
//!     Schedule::Interval(std::time::Duration::from_secs(3600)),
//!     dream_loop,
//!     Arc::new(|| {
//!         Context::new(
//!             LoopId::new(),
//!             CycleType::Time,
//!             StopCondition::max_iterations(1),
//!             MemoryDreamInput { dry_run: false },
//!         )
//!     }),
//! );
//! ```

use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use crate::loops::{Context, Loop, LoopResult};
use crate::memory::EpisodicMemory;

// ── Constants ───────────────────────────────────────────────────────

/// Default similarity threshold (0.0 – 1.0) above which entries are
/// considered duplicates.
const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.8;

// ── Input / Output types ────────────────────────────────────────────

/// Input for a single MemoryDream run.
#[derive(Debug, Clone, Default)]
pub struct MemoryDreamInput {
    /// When `true`, only report what would be done without making changes.
    pub dry_run: bool,
}

/// Outcome of a MemoryDream run.
#[derive(Debug, Clone)]
pub struct MemoryDreamOutput {
    /// Number of duplicate entries that were removed.
    pub duplicates_removed: usize,
    /// Number of groups created during consolidation.
    pub groups_created: usize,
    /// Summary of what was done.
    pub summary: String,
}

// ── MemoryDreamLoop ─────────────────────────────────────────────────

/// A loop that performs periodic memory deduplication and consolidation.
///
/// # Type parameters
///
/// * `S` — the summarizer callable: `Fn(&[(&str, &str)]) -> String`
///   receives a slice of `(input, output)` pairs and returns a summary.
pub struct MemoryDreamLoop<S> {
    /// Shared reference to the episodic memory store.
    episodic: std::sync::Arc<Mutex<EpisodicMemory>>,
    /// System prompt for memory consolidation (reserved for future LLM-powered use).
    _system_prompt: String,
    /// Similarity threshold for deduplication (0.0 – 1.0).
    similarity_threshold: f64,
    /// Summarizer callback that condenses grouped entries.
    summarizer: S,
    /// Execution counter.
    run_count: AtomicU64,
}

impl<S> MemoryDreamLoop<S>
where
    S: Fn(&[(&str, &str)]) -> String + Send + Sync,
{
    /// Create a new MemoryDream loop.
    ///
    /// * `episodic` — shared reference to the episodic memory
    /// * `system_prompt` — instruction for the consolidation process
    /// * `summarizer` — callable that receives `(input, output)` pairs and
    ///   returns a consolidated summary string
    pub fn new(
        episodic: std::sync::Arc<Mutex<EpisodicMemory>>,
        system_prompt: impl Into<String>,
        summarizer: S,
    ) -> Self {
        Self {
            episodic,
            _system_prompt: system_prompt.into(),
            similarity_threshold: DEFAULT_SIMILARITY_THRESHOLD,
            summarizer,
            run_count: AtomicU64::new(0),
        }
    }

    /// Set a custom similarity threshold.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Run the consolidation pipeline.
    fn consolidate(&self, dry_run: bool) -> MemoryDreamOutput {
        let mut memory = match self.episodic.lock() {
            Ok(m) => m,
            Err(_) => {
                return MemoryDreamOutput {
                    duplicates_removed: 0,
                    groups_created: 0,
                    summary: "Failed to lock episodic memory".into(),
                };
            }
        };

        if memory.is_empty() {
            return MemoryDreamOutput {
                duplicates_removed: 0,
                groups_created: 0,
                summary: "Memory is empty, nothing to consolidate".into(),
            };
        }

        let entries: Vec<crate::memory::EpisodicEntry> =
            memory.iter().into_iter().cloned().collect();

        // ── Step 1: Deduplication ──────────────────────────────────
        let mut to_remove: Vec<String> = Vec::new();
        for i in 0..entries.len() {
            if to_remove.contains(&entries[i].turn_id) {
                continue;
            }
            for j in (i + 1)..entries.len() {
                if to_remove.contains(&entries[j].turn_id) {
                    continue;
                }
                let sim = keyword_similarity(&entries[i], &entries[j]);
                if sim >= self.similarity_threshold {
                    // Keep the most recent entry
                    let older_id = if entries[i].timestamp > entries[j].timestamp {
                        &entries[j].turn_id
                    } else {
                        &entries[i].turn_id
                    };
                    if !to_remove.contains(older_id) {
                        to_remove.push(older_id.clone());
                    }
                }
            }
        }

        let duplicates_removed = to_remove.len();

        // ── Step 2: Reorganisation / grouping ──────────────────────
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut assigned: Vec<bool> = vec![false; entries.len()];

        for i in 0..entries.len() {
            if assigned[i] || to_remove.contains(&entries[i].turn_id) {
                continue;
            }
            let mut group = vec![i];
            assigned[i] = true;
            for j in (i + 1)..entries.len() {
                if assigned[j] || to_remove.contains(&entries[j].turn_id) {
                    continue;
                }
                let sim = keyword_similarity(&entries[i], &entries[j]);
                // Use a lower threshold for grouping (topic clustering)
                if sim >= self.similarity_threshold * 0.7 {
                    group.push(j);
                    assigned[j] = true;
                }
            }
            if group.len() > 1 {
                groups.push(group);
            }
        }

        let groups_created = groups.len();

        // ── Generate summary ───────────────────────────────────────
        let mut summary_parts: Vec<String> = Vec::new();

        if duplicates_removed > 0 && !dry_run {
            for id in &to_remove {
                memory.remove(id);
            }
            summary_parts.push(format!("Removed {duplicates_removed} duplicate entries"));
        } else if duplicates_removed > 0 {
            summary_parts.push(format!(
                "Would remove {duplicates_removed} duplicate entries (dry run)"
            ));
        }

        // Generate grouped summaries
        for (gi, group) in groups.iter().enumerate() {
            let pairs: Vec<(&str, &str)> = group
                .iter()
                .map(|&idx| {
                    let e = &entries[idx];
                    (e.input.as_str(), e.output.as_str())
                })
                .collect();

            let consolidated = (self.summarizer)(&pairs);
            if !consolidated.is_empty() {
                summary_parts.push(format!("Group {}: {consolidated}", gi + 1));
            }
        }

        if summary_parts.is_empty() {
            summary_parts.push("No consolidation needed".into());
        }

        let summary = summary_parts.join("; ");
        let total_removed = if dry_run { 0 } else { duplicates_removed };

        MemoryDreamOutput {
            duplicates_removed: total_removed,
            groups_created,
            summary,
        }
    }
}

/// Compute keyword similarity between two entries using Jaccard index
/// on their keyword sets (standalone, no generic parameter needed).
fn keyword_similarity(a: &crate::memory::EpisodicEntry, b: &crate::memory::EpisodicEntry) -> f64 {
    let set_a: std::collections::HashSet<&str> = a.keywords.iter().map(|s| s.as_str()).collect();
    let set_b: std::collections::HashSet<&str> = b.keywords.iter().map(|s| s.as_str()).collect();

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

#[async_trait::async_trait]
impl<S> Loop for MemoryDreamLoop<S>
where
    S: Fn(&[(&str, &str)]) -> String + Send + Sync + 'static,
{
    type Context = MemoryDreamInput;
    type State = ();
    type Output = MemoryDreamOutput;

    async fn execute(
        &self,
        ctx: Context<Self::Context>,
        _state: &mut Self::State,
    ) -> LoopResult<Self::Output> {
        let start = std::time::Instant::now();
        self.run_count.fetch_add(1, Ordering::SeqCst);

        let output = self.consolidate(ctx.input.dry_run);

        let elapsed = crate::loops::elapsed_ms(&start);
        LoopResult::success(output, 1, elapsed)
    }
}

// ── Default summarizer ──────────────────────────────────────────────

/// A simple rule-based summarizer that concatenates unique keywords and
/// counts the entries.
///
/// This is the fallback when no custom summarizer is provided.  For
/// production use, replace with an LLM-based summarizer.
pub fn default_summarizer(pairs: &[(&str, &str)]) -> String {
    let count = pairs.len();
    let mut all_keywords: Vec<String> = Vec::new();

    for (input, output) in pairs {
        let kws = EpisodicMemory::extract_keywords(input);
        all_keywords.extend(kws);
        let kws_out = EpisodicMemory::extract_keywords(output);
        all_keywords.extend(kws_out);
    }

    all_keywords.sort();
    all_keywords.dedup();

    // Truncate to a reasonable length
    if all_keywords.len() > 10 {
        all_keywords.truncate(10);
        all_keywords.push("…".into());
    }

    format!(
        "{count} related entries. Topics: {}",
        all_keywords.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::EpisodicMemory;
    use std::sync::Arc;

    fn make_entry(
        turn_id: &str,
        input: &str,
        output: &str,
        extra_kws: &[&str],
    ) -> crate::memory::EpisodicEntry {
        let mut keywords = EpisodicMemory::extract_keywords(input);
        keywords.extend(EpisodicMemory::extract_keywords(output));
        for kw in extra_kws {
            keywords.push(kw.to_string());
        }
        keywords.sort();
        keywords.dedup();

        crate::memory::EpisodicEntry {
            turn_id: turn_id.to_string(),
            timestamp: std::time::SystemTime::now(),
            input: input.to_string(),
            output: output.to_string(),
            tool_calls: vec![],
            keywords,
        }
    }

    #[test]
    fn test_keyword_similarity_identical() {
        let a = make_entry("t1", "deploy the app", "done", &[]);
        let b = make_entry("t2", "deploy the app", "done", &[]);
        let sim = keyword_similarity(&a, &b);
        assert!((sim - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_keyword_similarity_different() {
        let a = make_entry("t1", "deploy the app", "done", &[]);
        let b = make_entry("t2", "run tests", "passed", &[]);
        let sim = keyword_similarity(&a, &b);
        assert!(sim < 0.3);
    }

    #[test]
    fn test_consolidate_no_duplicates() {
        let mem = Arc::new(Mutex::new(EpisodicMemory::new()));
        {
            let mut m = mem.lock().unwrap();
            m.record(make_entry("t1", "deploy app", "deployed", &[]));
            m.record(make_entry("t2", "run tests", "passed", &[]));
        }

        let dream = MemoryDreamLoop::new(mem, "consolidate", default_summarizer);
        let output = dream.consolidate(false);
        assert_eq!(output.duplicates_removed, 0);
        assert!(output.summary.contains("No consolidation"));
    }

    #[test]
    fn test_consolidate_removes_duplicates() {
        let mem = Arc::new(Mutex::new(EpisodicMemory::new()));
        {
            let mut m = mem.lock().unwrap();
            m.record(make_entry(
                "t1",
                "deploy the app to production",
                "application deployed successfully",
                &[],
            ));
            m.record(make_entry(
                "t2",
                "deploy the app to production",
                "application deployed successfully",
                &[],
            ));
        }

        let dream = MemoryDreamLoop::new(mem, "consolidate", default_summarizer);
        let output = dream.consolidate(false);
        assert_eq!(output.duplicates_removed, 1);
        assert!(output.summary.contains("Removed"));
    }

    #[test]
    fn test_dry_run_does_not_remove() {
        let mem = Arc::new(Mutex::new(EpisodicMemory::new()));
        {
            let mut m = mem.lock().unwrap();
            m.record(make_entry(
                "t1",
                "deploy the app",
                "application deployed",
                &[],
            ));
            m.record(make_entry(
                "t2",
                "deploy the app",
                "application deployed",
                &[],
            ));
        }

        let dream = MemoryDreamLoop::new(mem.clone(), "consolidate", default_summarizer);
        let output = dream.consolidate(true);
        assert_eq!(output.duplicates_removed, 0);
        assert!(output.summary.contains("dry run"));

        // Entries should still be there
        let m = mem.lock().unwrap();
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn test_loop_execution() {
        let mem = Arc::new(Mutex::new(EpisodicMemory::new()));
        {
            let mut m = mem.lock().unwrap();
            m.record(make_entry("t1", "deploy app", "deployed", &[]));
        }

        let dream = MemoryDreamLoop::new(mem, "consolidate", default_summarizer);
        let ctx = Context::new(
            crate::loops::LoopId::new(),
            crate::loops::CycleType::Time,
            crate::loops::StopCondition::max_iterations(1),
            MemoryDreamInput::default(),
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(dream.execute(ctx, &mut ()));
        assert!(result.is_success());
        let output = result.output.unwrap();
        assert!(output.summary.contains("No consolidation"));
    }

    #[test]
    fn test_default_summarizer() {
        let pairs = vec![("deploy the app", "deployment complete")];
        let summary = default_summarizer(&pairs);
        assert!(summary.contains("deploy"));
        assert!(summary.contains("1 related"));
    }

    #[test]
    fn test_empty_memory_consolidate() {
        let mem = Arc::new(Mutex::new(EpisodicMemory::new()));
        let dream = MemoryDreamLoop::new(mem, "consolidate", default_summarizer);
        let output = dream.consolidate(false);
        assert_eq!(output.duplicates_removed, 0);
        assert!(output.summary.contains("empty"));
    }
}
