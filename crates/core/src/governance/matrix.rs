//! **Governance Matrix** — пер-агент матрица доступа к ресурсам.
//!
//! Определяет политики доступа для каждого агента по категориям:
//! Shell, FileRead, FileWrite, Network. Интегрируется с существующим
//! `AccessPolicyEvaluator` из модуля `sandbox`.

use crate::agent::tool::ToolCategory;
use crate::sandbox::{AccessPolicy, AccessPolicyEvaluator};
use std::collections::HashMap;

/// Одно правило в матрице governance для агента.
#[derive(Debug, Clone)]
pub struct GovernanceRule {
    /// Категория ресурса.
    pub category: ToolCategory,
    /// Политика доступа.
    pub policy: AccessPolicy,
    /// Причина/обоснование правила.
    pub reason: String,
}

/// Матрица governance — определяет правила для агента.
///
/// Каждый агент может иметь свою матрицу доступа, которая определяет,
/// какие действия разрешены (Allow), запрещены (Deny) или требуют
/// подтверждения пользователя (Ask).
#[derive(Debug, Clone)]
pub struct AgentGovernance {
    /// Имя агента.
    pub agent_name: String,
    /// Правила для категорий.
    rules: Vec<GovernanceRule>,
    /// Evaluator на основе правил.
    evaluator: AccessPolicyEvaluator,
}

impl AgentGovernance {
    /// Создать новую матрицу governance для агента.
    ///
    /// # Arguments
    /// * `agent_name` — имя агента
    /// * `default_policy` — политика по умолчанию для всех категорий
    #[must_use]
    pub fn new(agent_name: impl Into<String>, default_policy: AccessPolicy) -> Self {
        Self {
            agent_name: agent_name.into(),
            rules: Vec::new(),
            evaluator: AccessPolicyEvaluator::new(default_policy),
        }
    }

    /// Создать матрицу с политикой Allow по умолчанию.
    #[must_use]
    pub fn permissive(agent_name: impl Into<String>) -> Self {
        Self::new(agent_name, AccessPolicy::Allow)
    }

    /// Создать матрицу с политикой Deny по умолчанию (самая безопасная).
    #[must_use]
    pub fn restricted(agent_name: impl Into<String>) -> Self {
        Self::new(agent_name, AccessPolicy::Deny)
    }

    /// Добавить правило для категории.
    pub fn add_rule(
        &mut self,
        category: ToolCategory,
        policy: AccessPolicy,
        reason: impl Into<String>,
    ) {
        self.evaluator.set_category_policy(&category, policy);
        self.rules.push(GovernanceRule {
            category,
            policy,
            reason: reason.into(),
        });
    }

    /// Проверить, разрешён ли доступ к категории.
    ///
    /// # Arguments
    /// * `category` — категория инструмента
    /// * `session_id` — опциональный ID сессии для пер-сессионных переопределений
    #[must_use]
    pub fn evaluate(&self, category: &ToolCategory, session_id: Option<&str>) -> AccessPolicy {
        self.evaluator.evaluate(category, session_id)
    }

    /// Получить список всех правил.
    #[must_use]
    pub fn rules(&self) -> &[GovernanceRule] {
        &self.rules
    }

    /// Получить ссылку на внутренний evaluator.
    #[must_use]
    pub fn evaluator(&self) -> &AccessPolicyEvaluator {
        &self.evaluator
    }
}

/// Глобальный реестр матриц governance для всех агентов.
#[derive(Debug, Clone)]
pub struct GovernanceRegistry {
    /// Матрицы по имени агента.
    matrices: HashMap<String, AgentGovernance>,
    /// Политика по умолчанию для агентов без явной матрицы.
    default_for_unknown: AccessPolicy,
}

impl GovernanceRegistry {
    /// Создать пустой реестр.
    #[must_use]
    pub fn new(default_for_unknown: AccessPolicy) -> Self {
        Self {
            matrices: HashMap::new(),
            default_for_unknown,
        }
    }

    /// Зарегистрировать матрицу для агента.
    pub fn register(&mut self, governance: AgentGovernance) {
        let name = governance.agent_name.clone();
        self.matrices.insert(name, governance);
    }

    /// Получить матрицу для агента.
    #[must_use]
    pub fn get(&self, agent_name: &str) -> Option<&AgentGovernance> {
        self.matrices.get(agent_name)
    }

    /// Проверить доступ для агента.
    /// Если агент не имеет явной матрицы, используется `default_for_unknown`.
    #[must_use]
    pub fn evaluate(
        &self,
        agent_name: &str,
        category: &ToolCategory,
        session_id: Option<&str>,
    ) -> AccessPolicy {
        self.matrices
            .get(agent_name)
            .map(|g| g.evaluate(category, session_id))
            .unwrap_or(self.default_for_unknown)
    }

    /// Удалить матрицу агента.
    pub fn unregister(&mut self, agent_name: &str) {
        self.matrices.remove(agent_name);
    }

    /// Получить список всех зарегистрированных агентов.
    #[must_use]
    pub fn agents(&self) -> Vec<&str> {
        self.matrices.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_governance_default_allow() {
        let g = AgentGovernance::permissive("test-agent");
        assert_eq!(g.evaluate(&ToolCategory::Shell, None), AccessPolicy::Allow);
        assert_eq!(
            g.evaluate(&ToolCategory::Network, None),
            AccessPolicy::Allow
        );
    }

    #[test]
    fn test_agent_governance_restricted() {
        let mut g = AgentGovernance::restricted("restricted-agent");
        g.add_rule(
            ToolCategory::FileRead,
            AccessPolicy::Allow,
            "read access is needed",
        );

        assert_eq!(g.evaluate(&ToolCategory::Shell, None), AccessPolicy::Deny);
        assert_eq!(
            g.evaluate(&ToolCategory::FileRead, None),
            AccessPolicy::Allow
        );
    }

    #[test]
    fn test_governance_registry() {
        let mut registry = GovernanceRegistry::new(AccessPolicy::Deny);

        let mut g = AgentGovernance::permissive("helper");
        g.add_rule(ToolCategory::Shell, AccessPolicy::Deny, "no shell");
        registry.register(g);

        // helper: подчиняется своей матрице
        assert_eq!(
            registry.evaluate("helper", &ToolCategory::Shell, None),
            AccessPolicy::Deny
        );
        assert_eq!(
            registry.evaluate("helper", &ToolCategory::Network, None),
            AccessPolicy::Allow
        );

        // неизвестный агент: Deny по умолчанию
        assert_eq!(
            registry.evaluate("unknown", &ToolCategory::Network, None),
            AccessPolicy::Deny
        );
    }

    #[test]
    fn test_governance_session_override() {
        let g = AgentGovernance::restricted("session-agent");
        let _session = g.evaluator();
        // permissive session override would be handled by AccessPolicyEvaluator
        assert!(g.rules().is_empty());
    }
}
