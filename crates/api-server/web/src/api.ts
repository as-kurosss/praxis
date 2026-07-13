import type {
  ApiResponse, ProviderConfig, AgentDefinition, AgentSummary,
  ChatResponse, SessionSummary, Session,
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
