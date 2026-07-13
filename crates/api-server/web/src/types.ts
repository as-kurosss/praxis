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
  enabled?: boolean;
  model_id?: string | null;
  language?: string | null;
  auto_continue_retry?: number;
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

export interface Config {
  request_timeout_seconds: number
  owner_id: string
}

export interface Notification {
  kind: string
  message: string
  timestamp: string
}

export const BUILTIN_TOOLS = [
  { name: 'calculator', description: 'Performs arithmetic calculations' },
  { name: 'time', description: 'Gets the current time' },
  { name: 'shell', description: 'Executes shell commands' },
] as const;

// ── Observe types ────────────────────────────────────────

export interface TraceSummary {
  id: string;
  agent_id: string;
  session_id?: string | null;
  start_time: string;
  end_time?: string | null;
  status: string;
  token_count?: number | null;
  error?: string | null;
  span_count?: number | null;
}

export interface SpanSummary {
  id: string;
  trace_id: string;
  parent_span_id?: string | null;
  name: string;
  start_time: string;
  end_time?: string | null;
  metadata: unknown;
  token_count?: number | null;
}

export interface TraceDetail {
  id: string;
  agent_id: string;
  session_id?: string | null;
  start_time: string;
  end_time?: string | null;
  status: string;
  token_count?: number | null;
  error?: string | null;
  spans: SpanSummary[];
}

export interface MetricPoint {
  id: string;
  name: string;
  value: number;
  tags: unknown;
  timestamp: string;
}

export interface DashboardStats {
  total_traces: number;
  completed_traces: number;
  failed_traces: number;
  avg_latency_ms?: number | null;
  total_tokens: number;
}
