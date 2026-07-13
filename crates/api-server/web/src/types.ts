// ── Mirror of Rust praxis_core::registry types ──────────────────

export type ProviderKind = 'openai' | 'anthropic' | 'gemini' | 'ollama' | 'custom' | 'lm_studio';

export interface ProviderConfig {
  id: string;
  kind: ProviderKind;
  label: string;
  api_key: string;
  model: string;
  api_url?: string | null;
  notes?: string | null;
}

export interface ScrollConfig {
  type: 'truncate' | 'sliding_window' | 'no_op';
  max_messages?: number;
  window_size?: number;
}

export type ToolBinding =
  | { type: 'builtin'; name: string; enabled: boolean }
  | { type: 'custom'; name: string; description: string; schema: unknown; enabled: boolean };

export interface AgentDefinition {
  id: string;
  name: string;
  description?: string | null;
  provider_id: string;
  system_prompt: string;
  temperature?: number | null;
  max_tokens?: number | null;
  scroll_strategy: ScrollConfig;
  tools: ToolBinding[];
  created_at: string;
  updated_at: string;
}

export interface AgentSummary {
  id: string;
  name: string;
  description?: string | null;
  provider_id: string;
  system_prompt: string;
  tool_count: number;
  created_at: string;
  updated_at: string;
}

export interface ChatMessage {
  role: string;
  content?: string | null;
  reasoning_content?: string | null;
  tool_calls?: ToolCall[] | null;
  tool_call_id?: string | null;
  name?: string | null;
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: unknown;
}

export interface SessionSummary {
  id: string;
  agent_id: string;
  title?: string | null;
  message_count: number;
  created_at: string;
  updated_at: string;
  preview: string[];
}

export interface Session {
  id: string;
  agent_id: string;
  title?: string | null;
  messages: ChatMessage[];
  created_at: string;
  updated_at: string;
  message_count?: number;
}

export interface ApiResponse<T> {
  success: boolean;
  data?: T | null;
  error?: string | null;
}

export interface ChatResponse {
  session_id: string;
  message: string;
}

export interface StreamChunk {
  kind: 'token' | 'tool_call_start' | 'tool_call_end' | 'done' | 'error';
  data: string;
}

export const BUILTIN_TOOLS = [
  { name: 'calculator', description: 'Performs arithmetic calculations' },
  { name: 'time', description: 'Gets the current time' },
  { name: 'shell', description: 'Executes shell commands' },
] as const;

// ── Skills ──

export interface SkillDefinition {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  source_url?: string | null;
  version?: string | null;
  created_at: string;
}

export interface SkillImportRequest {
  url: string;
  name?: string;
}

// ── Settings ──

export interface AppSettings {
  default_scroll_strategy: ScrollConfig;
  default_model: string;
  default_temperature: number | null;
  default_max_tokens: number | null;
  theme: 'dark' | 'light';
  language: string;
}

// ── Memory ──

export interface MemorySearchResult {
  id: string;
  content: string;
  agent_id: string;
  session_id: string;
  similarity: number;
  created_at: string;
}

export interface MemoryEntry {
  id: string;
  content: string;
  agent_id: string;
  session_id: string;
  created_at: string;
  last_accessed_at: string;
}

export interface DreamConfig {
  enabled: boolean;
  interval_minutes: number;
  max_memories: number;
  consolidation_strategy: 'summary' | 'cluster' | 'none';
}

export interface RetentionConfig {
  max_memories: number;
  ttl_days: number;
  importance_threshold: number;
}

// ── Security ──

export type PolicyAction = 'allow' | 'deny' | 'ask';

export interface SecurityPolicy {
  id: string;
  name: string;
  description: string;
  action: PolicyAction;
  rules: SecurityRule[];
}

export interface SecurityRule {
  id: string;
  name: string;
  action: PolicyAction;
  pattern: string;
}

export interface SandboxConfig {
  enabled: boolean;
  docker_image: string;
  network_access: boolean;
  filesystem_access: 'read' | 'read_write' | 'none';
  timeout_seconds: number;
  memory_limit_mb: number;
}

export interface ShellEvasionRule {
  id: string;
  name: string;
  enabled: boolean;
  pattern: string;
  description: string;
}

// ── Observability ──

export interface TraceSpan {
  id: string;
  trace_id: string;
  name: string;
  start_time: string;
  end_time: string;
  duration_ms: number;
  status: 'ok' | 'error' | 'cancelled';
  metadata?: Record<string, unknown>;
}

export interface Trace {
  id: string;
  agent_id: string;
  session_id: string;
  spans: TraceSpan[];
  total_duration_ms: number;
  total_tokens: number;
  created_at: string;
}

export interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  cost: number;
}

// ── Logs ──

export interface LogEntry {
  timestamp: string;
  level: 'trace' | 'debug' | 'info' | 'warn' | 'error';
  message: string;
  target: string;
  fields?: Record<string, unknown>;
}
