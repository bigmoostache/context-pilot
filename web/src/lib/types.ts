// Types du contrat Pi ↔ navigateur — voir docs/nestor-web-contract.md.

export interface TokenBucket {
  cache_hit: number
  cache_miss: number
  output: number
  uncached_input: number
}

export interface ApiCheck {
  ok: boolean
  auth_ok: boolean
  streaming_ok: boolean
  tools_ok: boolean
  error: string | null
}

export interface WebStatus {
  stream_phase: 'idle' | 'receiving' | 'executing_tools'
  streaming_tool: { name: string; input_so_far: string } | null
  guard_rail_blocked: string | null
  last_stop_reason: string | null
  api_check_in_progress: boolean
  api_check: ApiCheck | null
  provider: string
  model: string
  secondary_provider: string
  secondary_model: string
  theme: string
  auto_continue: boolean
  reverie_enabled: boolean
  think_threshold: number | null
  max_cost: number | null
  cleaning_threshold: number
  cleaning_target: number
  context_used_tokens: number
  context_budget: number | null
  context_window: number
  session_tokens: TokenBucket
  tick_tokens: TokenBucket
  alive_breakpoints: number
  bp_positions_permille: number[]
  spine_notifications: number
}

export interface WebPanel {
  id: string
  uid: string | null
  kind: string
  name: string
  is_fixed: boolean
  selected: boolean
  token_count: number
  full_token_count: number
  page: number
  total_pages: number
  last_refresh_ms: number
}

export interface ActivePanel {
  id: string
  kind: string
  name: string
  content: string | null
  metadata: Record<string, unknown>
}

export interface ToolUse {
  id: string
  name: string
  input: unknown
}

export interface ToolResult {
  tool_use_id: string
  content: string
  tldr: string | null
  is_error: boolean
  tool_name: string
}

export interface WebMessage {
  id: string
  uid: string | null
  role: 'user' | 'assistant'
  kind: 'text' | 'tool_call' | 'tool_result'
  content: string
  status: 'full' | 'deleted' | 'detached'
  tool_uses: ToolUse[]
  tool_results: ToolResult[]
  timestamp_ms: number
}

export interface QuestionOption {
  label: string
  description: string
}

export interface Question {
  text: string
  header: string
  multi_select: boolean
  options: QuestionOption[]
}

export interface QuestionForm {
  tool_use_id: string
  questions: Question[]
}

export interface ModelEntry {
  id: string
  label: string
}

export interface ProviderEntry {
  id: string
  label: string
  models: ModelEntry[]
}

export interface ToolEntry {
  id: string
  name: string
  short_desc: string
  enabled: boolean
}

export interface WebMeta {
  themes: string[]
  providers: ProviderEntry[]
  tools: ToolEntry[]
  workspace: string
  version: string
}

export interface WebState {
  status: WebStatus
  panels: WebPanel[]
  active_panel: ActivePanel | null
  conversation: WebMessage[]
  question_form: QuestionForm | null
  input_draft: string
  meta: WebMeta
}

// Trames serveur → client
export type ServerFrame =
  | { t: 'snapshot'; state: WebState }
  | ({ t: 'delta' } & Partial<{
      status: WebStatus
      panels: WebPanel[]
      active_panel: ActivePanel | null
      question_form: QuestionForm | null
      input_draft: string
      conversation_upsert: WebMessage[]
      conversation_remove: string[]
    }>)
  | { t: 'append'; id: string; text: string }
  | { t: 'result'; req_id: string; data: unknown }
  | { t: 'error'; message: string }
  | { t: 'pong' }
  | { t: 'bye'; reason: string }

// Commandes client → serveur (face entrante du contrat)
export type WebCommand =
  | { cmd: 'submit'; text: string }
  | { cmd: 'stop' }
  | { cmd: 'select_panel'; id: string }
  | { cmd: 'clear_conversation' }
  | { cmd: 'new_context' }
  | { cmd: 'reset_costs' }
  | { cmd: 'reload' }
  | { cmd: 'set_provider'; scope: 'primary' | 'secondary'; provider: string }
  | { cmd: 'set_model'; scope: 'primary' | 'secondary'; model: string }
  | { cmd: 'set_theme'; theme: string }
  | { cmd: 'toggle_auto_continue' }
  | { cmd: 'toggle_reverie' }
  | { cmd: 'set_context_budget'; tokens: number | null }
  | { cmd: 'set_cleaning_threshold'; value: number }
  | { cmd: 'set_cleaning_target'; value: number }
  | { cmd: 'set_max_cost'; value: number | null }
  | { cmd: 'set_think_threshold'; value: number }
  | {
      cmd: 'answer_question'
      tool_use_id: string
      answers: { selected: number[]; other_text?: string }[]
    }
  | { cmd: 'dismiss_question'; tool_use_id: string }

export type WebQuery =
  | { q: 'list_dir'; dir: string; prefix: string }
  | { q: 'panel_content'; id: string }
  | { q: 'prompt_history'; limit?: number }
  | { q: 'index_status' }

export interface DirEntry {
  name: string
  is_dir: boolean
}

export interface DeviceInfo {
  id: string
  name: string
  created_ms: number
  last_seen_ms: number
}
