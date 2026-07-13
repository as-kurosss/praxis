import type {
  ApiResponse, ProviderConfig, AgentDefinition, AgentSummary,
  ChatResponse, SessionSummary, Session,
  SkillDefinition, AppSettings, MemorySearchResult,
  SecurityPolicy, Trace, LogEntry,
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

// ── Skills ──

export async function listSkills(): Promise<SkillDefinition[]> {
  return request('/api/skills');
}

export async function createSkill(body: Partial<SkillDefinition>): Promise<SkillDefinition> {
  return request('/api/skills', { method: 'POST', body: JSON.stringify(body) });
}

export async function deleteSkill(id: string): Promise<boolean> {
  return request(`/api/skills/${id}`, { method: 'DELETE' });
}

export async function toggleSkill(id: string, enabled: boolean): Promise<boolean> {
  return request(`/api/skills/${id}/toggle`, { method: 'POST', body: JSON.stringify({ enabled }) });
}

export async function importSkill(url: string): Promise<SkillDefinition> {
  return request('/api/skills/import', { method: 'POST', body: JSON.stringify({ url }) });
}

// ── Settings ──

export async function getSettings(): Promise<AppSettings> {
  return request('/api/settings');
}

export async function updateSettings(body: Partial<AppSettings>): Promise<AppSettings> {
  return request('/api/settings', { method: 'PUT', body: JSON.stringify(body) });
}

// ── Memory ──

export async function searchMemory(q: string): Promise<MemorySearchResult[]> {
  return request(`/api/memory/search?q=${encodeURIComponent(q)}`);
}

// ── Security ──

export async function listSecurityPolicies(): Promise<SecurityPolicy[]> {
  return request('/api/security/policies');
}

// ── Observability ──

export async function listTraces(): Promise<Trace[]> {
  return request('/api/observe/traces');
}

// ── Logs ──

export async function streamLogs(): Promise<LogEntry[]> {
  return request('/api/logs');
}

// ── Session Title ──

export async function getSessionTitle(id: string): Promise<Session> {
  return request(`/api/sessions/${id}`);
}

export async function updateSessionTitle(id: string, title: string): Promise<boolean> {
  return request(`/api/sessions/${id}/title`, { method: 'PUT', body: JSON.stringify({ title }) });
}
