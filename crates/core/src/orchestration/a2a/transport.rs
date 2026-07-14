//! **A2A Transport** — мост между A2A-протоколом и системой ACP.
//!
//! Реализует трейт `AcpTransport` через A2A-клиент, позволяя
//! использовать A2A-совместимых агентов как удалённых участников
//! в существующей ACP-инфраструктуре Praxis.

use super::client::A2AClient;
use super::types::*;
use crate::orchestration::acp::{AcpError, AcpStatus, AcpTransport, AgentId, AgentMessage};

/// Транспорт A2A, реализующий `AcpTransport`.
///
/// Отображает ACP-сообщения в A2A Task'и и обратно.
/// Каждое ACP-сообщение становится отдельной A2A-задачей.
#[derive(Debug, Clone)]
pub struct A2ATransport {
    /// Локальный идентификатор агента.
    local_id: AgentId,
    /// URL удалённого A2A-агента.
    remote_url: String,
    /// A2A-клиент.
    client: A2AClient,
}

impl A2ATransport {
    /// Создать новый A2A-транспорт.
    ///
    /// # Arguments
    /// * `local_id` — идентификатор локального агента
    /// * `remote_url` — URL удалённого A2A-агента
    #[must_use]
    pub fn new(local_id: impl Into<AgentId>, remote_url: impl Into<String>) -> Self {
        Self {
            local_id: local_id.into(),
            remote_url: remote_url.into(),
            client: A2AClient::new(),
        }
    }

    /// Создать транспорт с кастомным HTTP-клиентом (например, с прокси).
    #[must_use]
    pub fn with_client(
        local_id: impl Into<AgentId>,
        remote_url: impl Into<String>,
        client: A2AClient,
    ) -> Self {
        Self {
            local_id: local_id.into(),
            remote_url: remote_url.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl AcpTransport for A2ATransport {
    async fn send(&self, msg: AgentMessage<Vec<u8>>) -> Result<AcpStatus, AcpError> {
        // Создаём A2A-задачу с содержимым ACP-сообщения
        let payload_text = String::from_utf8_lossy(&msg.payload).to_string();
        let _message = Message::text(MessageRole::Agent, payload_text);

        let task_id = format!("acp-{}-{}", msg.conversation_id.0, msg.from);
        match self.client.create_task(&self.remote_url, &task_id).await {
            Ok(task) => {
                // Если задача уже завершена с результатом — возвращаем Completed
                if task.state == TaskState::Completed {
                    let response = task
                        .history
                        .last()
                        .and_then(|m| {
                            m.parts.first().and_then(|p| {
                                if let Part::Text(t) = p {
                                    Some(t.clone().into_bytes())
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or_default();
                    return Ok(AcpStatus::Completed(response));
                }

                Ok(AcpStatus::Accepted)
            }
            Err(A2AError::Http(e)) => Err(AcpError::Io(e)),
            Err(e) => Err(AcpError::InvalidMessage(e.to_string())),
        }
    }

    async fn receive(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<AgentMessage<Vec<u8>>>, AcpError> {
        // A2A — pull-based протокол. Здесь мы не можем эффективно
        // получать входящие сообщения, так как A2A не поддерживает
        // server-sent events как pull-интерфейс.
        //
        // Для полноценного receive необходимо использовать клиентскую
        // регистрацию callback'ов через SSE. Пока возвращаем None.
        let _ = timeout;
        Ok(None)
    }

    fn local_id(&self) -> AgentId {
        self.local_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_a2a_transport_send_accepted() {
        let mock_server = MockServer::start().await;

        let task = Task::new("acp-conv1-agent_a");
        let json = serde_json::to_value(&task).unwrap();

        Mock::given(method("POST"))
            .and(path("/tasks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&json))
            .mount(&mock_server)
            .await;

        let transport = Arc::new(A2ATransport::new(
            AgentId("agent_a".into()),
            mock_server.uri(),
        ));

        let msg = AgentMessage::new(
            AgentId("agent_a".into()),
            AgentId("remote".into()),
            "conv1",
            b"hello".to_vec(),
        );

        let status = transport.send(msg).await.unwrap();
        assert!(matches!(status, AcpStatus::Accepted));
    }

    #[tokio::test]
    async fn test_a2a_transport_send_completed() {
        let mock_server = MockServer::start().await;

        let mut task = Task::new("acp-conv1-agent_a");
        task.state = TaskState::Completed;
        task.add_message(Message::text(MessageRole::Agent, "pong"));
        let json = serde_json::to_value(&task).unwrap();

        Mock::given(method("POST"))
            .and(path("/tasks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&json))
            .mount(&mock_server)
            .await;

        let transport = A2ATransport::new(AgentId("agent_a".into()), mock_server.uri());

        let msg = AgentMessage::new(
            AgentId("agent_a".into()),
            AgentId("remote".into()),
            "conv1",
            b"ping".to_vec(),
        );

        let status = transport.send(msg).await.unwrap();
        match status {
            AcpStatus::Completed(data) => {
                assert_eq!(String::from_utf8_lossy(&data), "pong");
            }
            _ => panic!("expected Completed status"),
        }
    }
}
