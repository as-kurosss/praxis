//! **Approval System** — interactive user approval for tool execution.
//!
//! When an [AccessPolicy](super::AccessPolicy) is set to Ask for a
//! tool category, the [GovernedTool](super::GovernedTool) creates a
//! pending [ApprovalRequest] and suspends execution until a user
//! approves or denies it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Internal callback type for new pending approvals.
type ApprovalCallback = Arc<Mutex<Option<Box<dyn Fn(&ApprovalRequest) + Send + Sync>>>>;

/// Status of an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    /// Waiting for user decision.
    Pending,
    /// User approved execution.
    Approved,
    /// User denied execution.
    Denied,
}

/// A request for user approval before executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique identifier.
    pub id: String,
    /// Session ID this request belongs to.
    pub session_id: Option<String>,
    /// Name of the tool that requires approval.
    pub tool_name: String,
    /// JSON arguments for the tool call.
    pub tool_args: serde_json::Value,
    /// Human-readable explanation of why approval is needed.
    pub reason: String,
    /// Current status.
    pub status: ApprovalStatus,
    /// ISO-8601 timestamp when this request was created.
    pub created_at: String,
}

/// Thread-safe store for pending approval requests.
#[derive(Clone)]
pub struct PendingApprovalStore {
    requests: Arc<Mutex<HashMap<String, ApprovalRequest>>>,
    on_pending: ApprovalCallback,
}

impl std::fmt::Debug for PendingApprovalStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingApprovalStore")
            .field("pending_count", &self.pending_count())
            .finish()
    }
}

impl Default for PendingApprovalStore {
    fn default() -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
            on_pending: Arc::new(Mutex::new(None)),
        }
    }
}

impl PendingApprovalStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a callback invoked whenever a new pending request is added.
    pub fn set_on_pending(&self, cb: Box<dyn Fn(&ApprovalRequest) + Send + Sync>) {
        if let Ok(mut guard) = self.on_pending.lock() {
            *guard = Some(cb);
        }
    }

    /// Add a new approval request.
    pub fn add(&self, request: ApprovalRequest) {
        if let Ok(mut map) = self.requests.lock() {
            map.insert(request.id.clone(), request.clone());
        }
        // Fire the on-pending callback (outside the requests lock to avoid deadlocks)
        if let Ok(guard) = self.on_pending.lock()
            && let Some(ref cb) = *guard
            && let Ok(map) = self.requests.lock()
            && let Some(req) = map.get(&request.id)
        {
            cb(req);
        }
    }

    /// Get a request by ID.
    pub fn get(&self, id: &str) -> Option<ApprovalRequest> {
        self.requests
            .lock()
            .ok()
            .and_then(|map| map.get(id).cloned())
    }

    /// List all requests with the given status.
    pub fn list_by_status(&self, status: ApprovalStatus) -> Vec<ApprovalRequest> {
        self.requests
            .lock()
            .map(|map| {
                map.values()
                    .filter(|r| r.status == status)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all requests.
    pub fn list_all(&self) -> Vec<ApprovalRequest> {
        self.requests
            .lock()
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Approve a pending request.
    /// Returns true if the request existed and was pending.
    pub fn approve(&self, id: &str) -> bool {
        let mut map = match self.requests.lock() {
            Ok(m) => m,
            Err(_) => return false,
        };
        if let Some(r) = map.get_mut(id) {
            if r.status == ApprovalStatus::Pending {
                r.status = ApprovalStatus::Approved;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Deny a pending request.
    /// Returns true if the request existed and was pending.
    pub fn deny(&self, id: &str) -> bool {
        let mut map = match self.requests.lock() {
            Ok(m) => m,
            Err(_) => return false,
        };
        if let Some(r) = map.get_mut(id) {
            if r.status == ApprovalStatus::Pending {
                r.status = ApprovalStatus::Denied;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Remove resolved requests older than max_age_secs.
    pub fn cleanup(&self, max_age_secs: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut map) = self.requests.lock() {
            map.retain(|_, r| {
                if r.status == ApprovalStatus::Pending {
                    return true;
                }
                let created_secs =
                    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&r.created_at) {
                        ts.timestamp() as u64
                    } else {
                        now
                    };
                now.saturating_sub(created_secs) < max_age_secs
            });
        }
    }

    /// Number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.requests
            .lock()
            .map(|map| {
                map.values()
                    .filter(|r| r.status == ApprovalStatus::Pending)
                    .count()
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_store_empty() {
        let store = PendingApprovalStore::new();
        assert_eq!(store.pending_count(), 0);
        assert!(store.list_all().is_empty());
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn test_add_and_get_request() {
        let store = PendingApprovalStore::new();
        let req = ApprovalRequest {
            id: "req-1".into(),
            session_id: None,
            tool_name: "shell".into(),
            tool_args: serde_json::json!({"command": "ls"}),
            reason: "test".into(),
            status: ApprovalStatus::Pending,
            created_at: "2024-01-01T00:00:00Z".into(),
        };
        store.add(req.clone());
        assert_eq!(store.pending_count(), 1);
        let got = store.get("req-1");
        assert!(got.is_some());
        assert_eq!(got.unwrap().tool_name, "shell");
    }

    #[test]
    fn test_approve_request() {
        let store = PendingApprovalStore::new();
        store.add(ApprovalRequest {
            id: "req-1".into(),
            session_id: None,
            tool_name: "shell".into(),
            tool_args: serde_json::json!({}),
            reason: "test".into(),
            status: ApprovalStatus::Pending,
            created_at: String::new(),
        });
        assert!(store.approve("req-1"));
        assert!(!store.approve("req-1")); // already resolved
        let req = store.get("req-1").unwrap();
        assert_eq!(req.status, ApprovalStatus::Approved);
    }

    #[test]
    fn test_deny_pending_request() {
        let store = PendingApprovalStore::new();
        store.add(ApprovalRequest {
            id: "req-1".into(),
            session_id: None,
            tool_name: "network".into(),
            tool_args: serde_json::json!({}),
            reason: "test".into(),
            status: ApprovalStatus::Pending,
            created_at: String::new(),
        });
        assert!(store.deny("req-1"));
        assert!(!store.deny("req-1")); // already resolved
        assert_eq!(store.pending_count(), 0);
    }

    #[test]
    fn test_list_by_status() {
        let store = PendingApprovalStore::new();
        for i in 0..3 {
            store.add(ApprovalRequest {
                id: format!("req-{i}"),
                session_id: None,
                tool_name: "tool".into(),
                tool_args: serde_json::json!({}),
                reason: "test".into(),
                status: ApprovalStatus::Pending,
                created_at: String::new(),
            });
        }
        store.approve("req-0");
        assert_eq!(store.list_by_status(ApprovalStatus::Pending).len(), 2);
        assert_eq!(store.list_by_status(ApprovalStatus::Approved).len(), 1);
        assert_eq!(store.list_by_status(ApprovalStatus::Denied).len(), 0);
    }

    #[test]
    fn test_approve_deny_nonexistent() {
        let store = PendingApprovalStore::new();
        assert!(!store.approve("nope"));
        assert!(!store.deny("nope"));
    }

    #[test]
    fn test_cleanup_removes_old_resolved() {
        let store = PendingApprovalStore::new();
        store.add(ApprovalRequest {
            id: "pending".into(),
            session_id: None,
            tool_name: "t".into(),
            tool_args: serde_json::json!({}),
            reason: "".into(),
            status: ApprovalStatus::Pending,
            created_at: "1970-01-01T00:00:00Z".into(), // old but pending
        });
        store.add(ApprovalRequest {
            id: "old-resolved".into(),
            session_id: None,
            tool_name: "t".into(),
            tool_args: serde_json::json!({}),
            reason: "".into(),
            status: ApprovalStatus::Approved,
            created_at: "1970-01-01T00:00:00Z".into(),
        });
        store.cleanup(1); // 1 second max age
        assert!(
            store.get("pending").is_some(),
            "pending requests should survive cleanup"
        );
        assert!(
            store.get("old-resolved").is_none(),
            "old resolved should be cleaned"
        );
    }
}

/// Gate that combines policy evaluation and approval store.
#[derive(Debug, Clone)]
pub struct ApprovalGate {
    /// The policy evaluator to check access.
    pub evaluator: Arc<super::AccessPolicyEvaluator>,
    /// The store to create pending approval requests.
    pub store: Arc<PendingApprovalStore>,
}

impl ApprovalGate {
    /// Create a new approval gate.
    pub fn new(
        evaluator: Arc<super::AccessPolicyEvaluator>,
        store: Arc<PendingApprovalStore>,
    ) -> Self {
        Self { evaluator, store }
    }

    /// Check whether the tool can be executed.
    /// If Ask, create a pending approval request and return Err.
    pub fn check(
        &self,
        category: &crate::agent::tool::ToolCategory,
        tool_name: &str,
        tool_args: &serde_json::Value,
        session_id: Option<&str>,
    ) -> Result<(), crate::agent::tool::ToolError> {
        match self.evaluator.evaluate(category, session_id) {
            super::AccessPolicy::Allow => Ok(()),
            super::AccessPolicy::Deny => Err(crate::agent::tool::ToolError::AccessDenied {
                tool: tool_name.to_string(),
                reason: format!("access denied by policy for category {category:?}"),
            }),
            super::AccessPolicy::Ask => {
                let request = ApprovalRequest {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.map(String::from),
                    tool_name: tool_name.to_string(),
                    tool_args: tool_args.clone(),
                    reason: format!(
                        "Tool '{tool_name}' requires approval (category: {category:?})"
                    ),
                    status: ApprovalStatus::Pending,
                    created_at: crate::registry::timestamp(),
                };
                self.store.add(request);
                Err(crate::agent::tool::ToolError::AccessDenied {
                    tool: tool_name.to_string(),
                    reason: "requires-approval".into(),
                })
            }
        }
    }
}
