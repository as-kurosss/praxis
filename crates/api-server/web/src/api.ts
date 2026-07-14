import type {
  ApiResponse, ProviderConfig, AgentDefinition, AgentSummary,
  ChatResponse, SessionSummary, Session, Config, Notification,
  TraceSummary, TraceDetail, MetricPoint, DashboardStats,
  ApprovalRequest, McpServerConfig,
} from './types';

class ApiError extends Error {
  constructor(msg: string) { super(msg); this.name = 'ApiError'; }
}

async function request<T>(path: string, opts: RequestInit = {}): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json', ...opts.headers as Record<string, string> },
    ...opts,
  });
  const json: ApiResponse<T> = await res.json();
  if (!json.success) throw new ApiError(json.error || 'API error');
  return json.data as T;
}

// ── Providers ──

export async function listProviders(): Promise<ProviderConfig[]> {
  return request('/api/providers');
}

export async function createProvider(body: Partial<ProviderConfig>): Promise<ProviderConfig> {
  return request('/api/providers', { method: 'POST', body: JSON.stringify(body) });
}

export async function updateProvider(id: string, body: Partial<ProviderConfig>): Promise<ProviderConfig> {
  return request(`/api/providers/${id}`, { method: 'PUT', body: JSON.stringify(body) });
}

export async function deleteProvider(id: string): Promise<boolean> {
  return request(`/api/providers/${id}`, { method: 'DELETE' });
}

// ── Agents ──

export async function listAgents(): Promise<AgentSummary[]> {
  return request('/api/agents');
}

export async function getAgent(id: string): Promise<AgentDefinition> {
  return request(`/api/agents/${id}`);
}

export async function createAgent(body: Partial<AgentDefinition>): Promise<AgentDefinition> {
  return request('/api/agents', { method: 'POST', body: JSON.stringify(body) });
}

export async function updateAgent(id: string, body: Partial<AgentDefinition>): Promise<AgentDefinition> {
  return request(`/api/agents/${id}`, { method: 'PUT', body: JSON.stringify(body) });
}

export async function deleteAgent(id: string): Promise<boolean> {
  return request(`/api/agents/${id}`, { method: 'DELETE' });
}

// ── Chat ──

export async function chatNonStreaming(
  agentId: string, message: string, sessionId?: string | null,
): Promise<ChatResponse> {
  return request(`/api/agents/${agentId}/chat`, {
    method: 'POST',
    body: JSON.stringify({ message, session_id: sessionId || null }),
  });
}

// ── Sessions ──

export async function listSessions(agentId: string): Promise<SessionSummary[]> {
  return request(`/api/agents/${agentId}/sessions`);
}

export async function getSession(id: string): Promise<Session> {
  return request(`/api/sessions/${id}`);
}

export async function deleteSession(id: string): Promise<boolean> {
  return request(`/api/sessions/${id}`, { method: 'DELETE' });
}

// ── Config ──

export async function getConfig(): Promise<Config> {
  return request('/api/config');
}

// ── Notifications ──

export async function getNotifications(): Promise<Notification[]> {
  return request('/api/notifications');
}

// ── Skills ──

export async function listSkills(): Promise<unknown[]> {
  return request('/api/skills');
}

// ── Memory ──

export async function searchMemory(q: string): Promise<unknown[]> {
  return request(`/api/memory/search?q=${encodeURIComponent(q)}`);
}

// ── Security ──

export async function getSecurityPolicies(): Promise<unknown> {
  return request('/api/security/policies');
}

// ── Observe ──

export async function listTraces(): Promise<TraceSummary[]> {
  return request('/api/observe/traces');
}

export async function getTraceDetail(id: string): Promise<TraceDetail> {
  return request(`/api/observe/traces/${id}`);
}

export async function getTraceSpans(id: string): Promise<unknown[]> {
  return request(`/api/observe/traces/${id}/spans`);
}

export async function listMetrics(): Promise<MetricPoint[]> {
  return request('/api/observe/metrics');
}

export async function getDashboardStats(): Promise<DashboardStats> {
  return request('/api/observe/stats');
}

// ── Logs ──

export async function streamLogs(): Promise<unknown[]> {
  return request('/api/logs');
}

// ── Settings ──

export async function getSettings(): Promise<unknown> {
  return request('/api/settings');
}

export async function updateSettings(body: unknown): Promise<unknown> {
  return request('/api/settings', { method: 'PUT', body: JSON.stringify(body) });
}

// ── Session title ──

export async function updateSessionTitle(id: string, title: string): Promise<Session> {
  return request(`/api/sessions/${id}/title`, { method: 'PUT', body: JSON.stringify({ title }) });
}

// ── MCP Servers ──

export async function listMcpServers(): Promise<McpServerConfig[]> {
  return request('/api/mcp-servers');
}

export async function createMcpServer(body: Partial<McpServerConfig>): Promise<McpServerConfig> {
  return request('/api/mcp-servers', { method: 'POST', body: JSON.stringify(body) });
}

export async function updateMcpServer(name: string, body: Partial<McpServerConfig>): Promise<McpServerConfig> {
  return request(`/api/mcp-servers/${encodeURIComponent(name)}`, { method: 'PUT', body: JSON.stringify(body) });
}

export async function deleteMcpServer(name: string): Promise<boolean> {
  return request(`/api/mcp-servers/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

// ── Approvals ──

export async function listPendingApprovals(): Promise<ApprovalRequest[]> {
  return request('/api/approvals/pending');
}

export async function approveApproval(id: string): Promise<boolean> {
  return request(`/api/approvals/${id}/approve`, { method: 'POST' });
}

export async function denyApproval(id: string): Promise<boolean> {
  return request(`/api/approvals/${id}/deny`, { method: 'POST' });
}
