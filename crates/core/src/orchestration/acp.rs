//! **ACP — Agent Communication Protocol** — cross-process and cross-network
//! message passing between agents.
//!
//! ACP provides:
//! * A message envelope ([`AgentMessage`]) with routing and TTL
//! * An abstract transport trait ([`AcpTransport`])
//! * [`RemoteAgent`] — a [`Loop`] that communicates via ACP
//! * [`StdioTransport`] — spawns an external process and uses stdio for communication

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::marker::PhantomData;
use std::time::{Duration, SystemTime};

/// ACP-specific errors.
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    /// The transport connection was closed.
    #[error("ACP connection closed")]
    ConnectionClosed,
    /// The transport timed out.
    #[error("ACP timeout")]
    Timeout,
    /// A message was received but its TTL has expired.
    #[error("ACP message expired")]
    MessageExpired,
    /// Failed to spawn the external process.
    #[error("Failed to spawn external process: {0}")]
    Spawn(String),
    /// I/O error during stdio communication.
    #[error("ACP I/O error: {0}")]
    Io(String),
    /// Invalid JSON message received.
    #[error("Invalid ACP message: {0}")]
    InvalidMessage(String),
}

/// Unique identifier for an agent in the ACP network.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for AgentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a multi-turn conversation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversationId(pub String);

impl From<String> for ConversationId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ConversationId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A message sent between agents over ACP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage<Payload> {
    /// Sender agent ID.
    pub from: AgentId,
    /// Recipient agent ID.
    pub to: AgentId,
    /// Conversation this message belongs to.
    pub conversation_id: ConversationId,
    /// Message payload (arbitrary bytes).
    pub payload: Payload,
    /// Time-to-live — message expires after this duration.
    pub ttl: Duration,
    /// When the message was created (set automatically in `new()`).
    pub sent_at: SystemTime,
}

impl<Payload> AgentMessage<Payload> {
    /// Create a new ACP message with `sent_at` set to now.
    pub fn new(
        from: impl Into<AgentId>,
        to: impl Into<AgentId>,
        conversation_id: impl Into<ConversationId>,
        payload: Payload,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            conversation_id: conversation_id.into(),
            payload,
            ttl: Duration::from_secs(60),
            sent_at: SystemTime::now(),
        }
    }

    /// Set a custom TTL.
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }
}

/// Status returned after sending an ACP message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AcpStatus {
    /// Message accepted for delivery.
    Accepted,
    /// Message is being processed by the recipient.
    Processing,
    /// Processing completed with a result payload.
    Completed(Vec<u8>),
    /// Processing failed with an error.
    Failed(String),
}

/// Abstract transport layer for ACP messages.
///
/// Implementations can use in-memory channels, TCP, stdio, or any other
/// communication mechanism.
#[async_trait::async_trait]
pub trait AcpTransport: Send + Sync {
    /// Send a message to another agent.
    async fn send(&self, msg: AgentMessage<Vec<u8>>) -> Result<AcpStatus, AcpError>;

    /// Receive the next pending message, waiting up to `timeout`.
    ///
    /// Returns `Ok(None)` on timeout, `Err(AcpError::ConnectionClosed)` when
    /// the remote end is gone, and `Ok(Some(msg))` on success.
    async fn receive(&self, timeout: Duration) -> Result<Option<AgentMessage<Vec<u8>>>, AcpError>;

    /// The local agent ID for this transport endpoint.
    fn local_id(&self) -> AgentId;
}

/// In-memory transport for ACP messages.
///
/// Uses [`tokio::sync::mpsc`] channels — suitable for in-process
/// multi-agent communication. Expired messages (beyond TTL) are
/// silently skipped during [`receive`](InMemoryTransport::receive).
#[derive(Debug)]
pub struct InMemoryTransport {
    local_id: AgentId,
    tx: tokio::sync::mpsc::Sender<AgentMessage<Vec<u8>>>,
    rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<AgentMessage<Vec<u8>>>>,
}

impl InMemoryTransport {
    /// Create a new in-memory transport pair with the default buffer (256).
    ///
    /// Returns two transports connected to each other. Messages sent from
    /// `a` are received by `b` and vice versa.
    pub fn pair(id_a: impl Into<AgentId>, id_b: impl Into<AgentId>) -> (Self, Self) {
        Self::pair_with_buffer(id_a, id_b, 256)
    }

    /// Create a transport pair with a custom channel buffer size.
    pub fn pair_with_buffer(
        id_a: impl Into<AgentId>,
        id_b: impl Into<AgentId>,
        buffer: usize,
    ) -> (Self, Self) {
        let (tx_ab, rx_ab) = tokio::sync::mpsc::channel(buffer);
        let (tx_ba, rx_ba) = tokio::sync::mpsc::channel(buffer);

        let a = Self {
            local_id: id_a.into(),
            tx: tx_ab,
            rx: tokio::sync::Mutex::new(rx_ba),
        };

        let b = Self {
            local_id: id_b.into(),
            tx: tx_ba,
            rx: tokio::sync::Mutex::new(rx_ab),
        };

        (a, b)
    }
}

#[async_trait::async_trait]
impl AcpTransport for InMemoryTransport {
    async fn send(&self, msg: AgentMessage<Vec<u8>>) -> Result<AcpStatus, AcpError> {
        self.tx
            .send(msg)
            .await
            .map(|_| AcpStatus::Accepted)
            .map_err(|_| AcpError::ConnectionClosed)
    }

    async fn receive(&self, timeout: Duration) -> Result<Option<AgentMessage<Vec<u8>>>, AcpError> {
        let mut rx = self.rx.lock().await;
        loop {
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(msg)) => {
                    // Enforce TTL: skip expired messages
                    if let Ok(age) = msg.sent_at.elapsed() {
                        if age < msg.ttl {
                            return Ok(Some(msg));
                        }
                        // Message expired, loop to try the next one
                    } else {
                        // Clock went backwards — accept to avoid data loss
                        return Ok(Some(msg));
                    }
                }
                Ok(None) => return Err(AcpError::ConnectionClosed),
                Err(_) => return Ok(None), // timeout
            }
        }
    }

    fn local_id(&self) -> AgentId {
        self.local_id.clone()
    }
}

/// Stdio-based transport for ACP messages.
///
/// Spawns an external process (e.g. Claude Code, Codex) and communicates
/// with it over JSON-delimited stdin/stdout messages.
///
/// # Supported runners
/// * `claude` — Claude Code CLI (`claude`)
/// * `codex` — Codex CLI (`codex`)
/// * `opencode` — OpenCode CLI (`opencode`)
/// * `qwen` — Qwen Code CLI (`qwen`)
pub struct StdioTransport {
    local_id: AgentId,
    child: std::sync::Mutex<Option<tokio::process::Child>>,
    stdin: tokio::sync::Mutex<Option<tokio::process::ChildStdin>>,
    reader_handle: Option<tokio::task::JoinHandle<()>>,
    rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Result<AgentMessage<Vec<u8>>, AcpError>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl std::fmt::Debug for StdioTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioTransport")
            .field("local_id", &self.local_id)
            .finish()
    }
}

impl StdioTransport {
    /// Create a new stdio transport that spawns an external agent process.
    ///
    /// `runner` must be one of `"claude"`, `"codex"`, `"opencode"`, `"qwen"`.
    /// If the runner binary is not on `$PATH`, it may also be an absolute path.
    pub fn spawn(
        local_id: impl Into<AgentId>,
        runner: &str,
        args: &[&str],
    ) -> Result<Self, AcpError> {
        let binary = runner;

        let mut child = tokio::process::Command::new(binary)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AcpError::Spawn(format!("cannot spawn '{binary}': {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpError::Spawn("failed to capture stdin".into()))?;
        let mut child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| AcpError::Spawn("failed to capture stdout".into()))?;

        // Spawn a background task that reads lines from stdout and sends them over a channel
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<AgentMessage<Vec<u8>>, AcpError>>(256);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let reader = tokio::io::BufReader::new(&mut child_stdout);
            let mut lines = reader.lines();
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    line = lines.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                let trimmed = line.trim().to_string();
                                if trimmed.is_empty() {
                                    continue;
                                }
                                let msg = match serde_json::from_str(&trimmed) {
                                    Ok(m) => Ok(m),
                                    Err(e) => Err(AcpError::InvalidMessage(e.to_string())),
                                };
                                if tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {
                                let _ = tx.send(Err(AcpError::ConnectionClosed)).await;
                                break;
                            }
                            Err(e) => {
                                let _ = tx.send(Err(AcpError::Io(e.to_string()))).await;
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            local_id: local_id.into(),
            child: std::sync::Mutex::new(Some(child)),
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            reader_handle: Some(handle),
            rx: tokio::sync::Mutex::new(rx),
            shutdown: Some(shutdown_tx),
        })
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Send shutdown signal to the reader task
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        // Abort the reader background task
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
        // Kill the child process
        if let Ok(mut child_lock) = self.child.lock()
            && let Some(mut child) = child_lock.take()
        {
            let _ = child.start_kill();
        }
    }
}

#[async_trait::async_trait]
impl AcpTransport for StdioTransport {
    async fn send(&self, msg: AgentMessage<Vec<u8>>) -> Result<AcpStatus, AcpError> {
        let mut stdin = self.stdin.lock().await;
        let stdin = stdin.as_mut().ok_or(AcpError::ConnectionClosed)?;
        use tokio::io::AsyncWriteExt;
        let json = serde_json::to_vec(&msg).map_err(|e| AcpError::InvalidMessage(e.to_string()))?;
        let mut payload = json;
        payload.push(b'\n');
        stdin
            .write_all(&payload)
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        Ok(AcpStatus::Accepted)
    }

    async fn receive(&self, timeout: Duration) -> Result<Option<AgentMessage<Vec<u8>>>, AcpError> {
        let mut rx = self.rx.lock().await;
        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(Ok(msg))) => Ok(Some(msg)),
            Ok(Some(Err(e))) => Err(e),
            Ok(None) => Err(AcpError::ConnectionClosed),
            Err(_) => Ok(None), // timeout
        }
    }

    fn local_id(&self) -> AgentId {
        self.local_id.clone()
    }
}

/// A [`Loop`] that communicates with a remote agent via ACP.
///
/// Serializes the context (as JSON), sends it over the transport, and waits
/// for a response (also JSON). Generic over context/output types that
/// implement [`Serialize`] + [`Deserialize`].
pub struct RemoteAgent<T: AcpTransport, C: Serialize, O: DeserializeOwned> {
    transport: std::sync::Arc<T>,
    target: AgentId,
    _phantom: PhantomData<(C, O)>,
}

impl<T: AcpTransport, C: Serialize, O: DeserializeOwned> RemoteAgent<T, C, O> {
    /// Create a remote agent proxy.
    pub fn new(transport: std::sync::Arc<T>, target: impl Into<AgentId>) -> Self {
        Self {
            transport,
            target: target.into(),
            _phantom: PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T, C, O> crate::loops::Loop for RemoteAgent<T, C, O>
where
    T: AcpTransport + 'static,
    C: Serialize + Send + Sync + 'static,
    O: DeserializeOwned + Send + Sync + 'static,
{
    type Context = C;
    type State = ();
    type Output = O;

    async fn execute(
        &self,
        ctx: crate::loops::Context<Self::Context>,
        _state: &mut Self::State,
    ) -> crate::loops::LoopResult<Self::Output> {
        let start = std::time::Instant::now();

        // Serialize context input as JSON bytes
        let payload = match serde_json::to_vec(&ctx.input) {
            Ok(bytes) => bytes,
            Err(e) => {
                return crate::loops::LoopResult::failure(
                    format!("ACP serialization error: {e}"),
                    1,
                    crate::loops::elapsed_ms(&start),
                );
            }
        };

        let msg = AgentMessage::new(
            self.transport.local_id(),
            self.target.clone(),
            ctx.id.to_string(),
            payload,
        );

        // Send the message
        let status = match self.transport.send(msg).await {
            Ok(s) => s,
            Err(e) => {
                return crate::loops::LoopResult::failure(
                    format!("ACP send error: {e}"),
                    1,
                    crate::loops::elapsed_ms(&start),
                );
            }
        };

        match status {
            AcpStatus::Accepted => {
                let timeout = Duration::from_secs(30);
                match self.transport.receive(timeout).await {
                    Ok(Some(response)) => match serde_json::from_slice(&response.payload) {
                        Ok(output) => crate::loops::LoopResult::success(
                            output,
                            1,
                            crate::loops::elapsed_ms(&start),
                        ),
                        Err(e) => crate::loops::LoopResult::failure(
                            format!("ACP deserialization error: {e}"),
                            1,
                            crate::loops::elapsed_ms(&start),
                        ),
                    },
                    Ok(None) => crate::loops::LoopResult::failure(
                        "timeout waiting for ACP response".to_string(),
                        1,
                        crate::loops::elapsed_ms(&start),
                    ),
                    Err(e) => crate::loops::LoopResult::failure(
                        format!("ACP receive error: {e}"),
                        1,
                        crate::loops::elapsed_ms(&start),
                    ),
                }
            }
            AcpStatus::Failed(reason) => crate::loops::LoopResult::failure(
                format!("remote agent failed: {reason}"),
                1,
                crate::loops::elapsed_ms(&start),
            ),
            other => crate::loops::LoopResult::failure(
                format!("unexpected ACP status: {other:?}"),
                1,
                crate::loops::elapsed_ms(&start),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::{Context, CycleType, Loop, LoopId, StopCondition};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_in_memory_transport_roundtrip() {
        let (transport_a, transport_b) = InMemoryTransport::pair("agent_a", "agent_b");
        let transport_a = Arc::new(transport_a);
        let transport_b = Arc::new(transport_b);

        // Spawn a task that receives and responds
        let b_clone = Arc::clone(&transport_b);
        tokio::spawn(async move {
            let msg = b_clone
                .receive(Duration::from_secs(5))
                .await
                .unwrap()
                .unwrap();
            let response = format!("echo: {}", String::from_utf8_lossy(&msg.payload));
            let reply = AgentMessage::new(
                msg.to.clone(),
                msg.from,
                msg.conversation_id,
                response.into_bytes(),
            );
            let _ = b_clone.send(reply).await;
        });

        // Send message from A to B and wait for response
        let msg = AgentMessage::new(
            AgentId("agent_a".into()),
            AgentId("agent_b".into()),
            ConversationId("conv_1".into()),
            b"hello".to_vec(),
        );
        let status = transport_a.send(msg).await.unwrap();
        assert!(matches!(status, AcpStatus::Accepted));

        // Receive the response
        let response = transport_a.receive(Duration::from_secs(5)).await.unwrap();
        assert!(response.is_some());
        let response = response.unwrap();
        let content = String::from_utf8_lossy(&response.payload);
        assert_eq!(content, "echo: hello");
    }

    #[tokio::test]
    async fn test_remote_agent_via_acp() {
        let (transport_a, transport_b) = InMemoryTransport::pair("client", "server");
        let transport_a = Arc::new(transport_a);
        let transport_b = Arc::new(transport_b);

        // Spawn a server that echoes
        let srv = Arc::clone(&transport_b);
        tokio::spawn(async move {
            let msg = srv.receive(Duration::from_secs(5)).await.unwrap().unwrap();
            let received: String = serde_json::from_slice(&msg.payload).unwrap();
            let response = format!("pong: {received}");
            let reply = AgentMessage::new(
                msg.to.clone(),
                msg.from,
                msg.conversation_id,
                serde_json::to_vec(&response).unwrap(),
            );
            let _ = srv.send(reply).await;
        });

        let remote = RemoteAgent::<_, String, String>::new(transport_a, "server");
        let ctx = Context::new(
            LoopId::new(),
            CycleType::Turn,
            StopCondition::max_iterations(1),
            "ping".to_string(),
        );

        let result = remote.execute(ctx, &mut ()).await;
        assert!(result.is_success());
        assert_eq!(result.output, Some("pong: ping".to_string()));
    }

    #[tokio::test]
    async fn test_ttl_expired_skipped() {
        let (ta, tb) = InMemoryTransport::pair("a", "b");
        let ta = Arc::new(ta);
        let tb = Arc::new(tb);

        // Send a message with 0 TTL (will expire immediately)
        let msg = AgentMessage::new("a", "b", "conv_1", b"expired".to_vec())
            .with_ttl(Duration::from_secs(0));
        ta.send(msg).await.unwrap();

        // Send a valid message
        let msg = AgentMessage::new("a", "b", "conv_1", b"valid".to_vec())
            .with_ttl(Duration::from_secs(60));
        ta.send(msg).await.unwrap();

        // Receive should skip the expired one and return the valid one
        let response = tb.receive(Duration::from_secs(5)).await.unwrap();
        assert!(response.is_some());
        assert_eq!(response.unwrap().payload, b"valid");
    }

    #[tokio::test]
    async fn test_pair_with_buffer() {
        let (ta, _tb) = InMemoryTransport::pair_with_buffer("a", "b", 16);
        // Send 16 messages (fills the buffer)
        for i in 0..16 {
            let msg = AgentMessage::new("a", "b", "conv", vec![i]);
            ta.send(msg).await.unwrap();
        }
        // 17th would block, but we don't test that here
    }
}
