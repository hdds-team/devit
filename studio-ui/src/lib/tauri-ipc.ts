/**
 * Tauri IPC wrapper for devit-studio (Tauri 2.x)
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';

// Types
export interface FileContent {
  path: string;
  content: string;
  language: string | null;
}

export interface FileEntry {
  path: string;
  name: string;
  is_dir: boolean;
  children: FileEntry[] | null;
}

export interface Symbol {
  name: string;
  kind: string;
  line: number;
}

export interface Provider {
  id: string;
  name: string;
  kind: 'local' | 'cloud';
  available: boolean;
}

export interface ModelInfo {
  name: string;
  size: string;
  modified: string;
}

// Context compression types
export interface TopicSummary {
  title: string;
  summary: string;
  token_count: number;
  original_message_count: number;
  created_at: string;
}

export interface CompactMessage {
  role: string;
  content: string;
}

export interface CompactResult {
  topic: TopicSummary;
  remaining_messages: CompactMessage[];
  compressed_count: number;
}

export interface ContextEstimate {
  total_tokens: number;
  max_tokens: number;
  usage_percent: number;
  needs_compression: boolean;
}

export interface GhostUpdate {
  session_id: string;
  position: { line: number; column: number };
  delta: string;
  pending_text: string;
  done: boolean;
}

export interface Settings {
  theme: string;
  ghost_accept_mode: 'popup' | 'chat';
  default_provider: string;
  font_size: number;
  font_family: string;
  chat_font_size: number;
  chat_font_family: string;
  default_model?: string;
  system_prompt?: string;
  llamacpp_url: string;
  lmstudio_url: string;
}

// Editor commands
export const editor = {
  openFile: (path: string) => invoke<FileContent>('open_file', { path }),
  reloadFile: (path: string) => invoke<FileContent>('reload_file', { path }),
  saveFile: (path: string, content: string) => invoke<void>('save_file', { path, content }),
  getSymbols: (path: string) => invoke<Symbol[]>('get_symbols', { path }),
};

// File watcher commands
export const watcher = {
  watch: (path: string) => invoke<void>('watch_file', { path }),
  unwatch: (path: string) => invoke<void>('unwatch_file', { path }),
  list: () => invoke<string[]>('list_watched_files'),
};

// Search result type
export interface SearchResult {
  path: string;
  line: number;
  column: number;
  text: string;
  match_text: string;
}

// Git diff line type
export interface GitDiffLine {
  line: number;
  kind: 'added' | 'modified' | 'deleted';
}

// Workspace commands
export const workspace = {
  openFolder: (path: string) => invoke<FileEntry>('open_folder', { path }),
  listFiles: (path: string, maxDepth?: number) =>
    invoke<FileEntry>('list_files', { path, max_depth: maxDepth }),
  getWorkspace: () => invoke<FileEntry | null>('get_workspace'),
  getGitStatus: () => invoke<any>('get_git_status'),
  getFileDiff: (path: string) => invoke<GitDiffLine[]>('get_file_diff', { path }),
  searchInFiles: (pattern: string, glob?: string, caseSensitive?: boolean) =>
    invoke<SearchResult[]>('search_in_files', { pattern, glob, case_sensitive: caseSensitive }),
};

// LLM commands
export const llm = {
  streamChat: (messages: any[]) => invoke<void>('stream_chat', { messages }),
  cancelStream: () => invoke<void>('cancel_stream'),
  listProviders: () => invoke<Provider[]>('list_providers'),
  setProvider: (providerId: string) => invoke<void>('set_provider', { providerId }),
  setModel: (modelName: string) => invoke<void>('set_model', { modelName }),
  listModels: (providerId?: string) => invoke<ModelInfo[]>('list_models', { providerId }),
  getDefaultSystemPrompt: () => invoke<string>('get_default_system_prompt'),
  // Context compression
  compactContext: (messages: any[], force?: boolean) => invoke<CompactResult>('compact_context', { messages, force }),
  estimateTokens: (messages: any[]) => invoke<ContextEstimate>('estimate_context_tokens', { messages }),
  // V2: uses real server context size
  estimateTokensV2: (messages: any[]) => invoke<ContextEstimate>('estimate_context_tokens_v2', { messages }),
  getServerProps: () => invoke<ServerProps>('get_server_props'),
};

// Server properties
export interface ServerProps {
  n_ctx: number;
  model_alias?: string;
  total_slots?: number;
}

// Ghost cursor commands
export const ghost = {
  startEdit: (request: any) => invoke<string>('start_ghost_edit', { request }),
  accept: (sessionId: string) => invoke<string>('accept_ghost', { sessionId }),
  reject: (sessionId: string) => invoke<void>('reject_ghost', { sessionId }),
  getState: (sessionId: string) => invoke<any>('get_ghost_state', { sessionId }),
};

// LSP commands
export const lsp = {
  start: (language: string, workspacePath: string) => 
    invoke<any>('start_lsp', { language, workspacePath }),
  stop: (language: string) => invoke<void>('stop_lsp', { language }),
  getCompletions: (filePath: string, line: number, column: number) =>
    invoke<any[]>('get_completions', { filePath, line, column }),
  getHover: (filePath: string, line: number, column: number) =>
    invoke<any>('get_hover', { filePath, line, column }),
};

// Settings commands
export const settings = {
  get: () => invoke<Settings>('get_settings'),
  set: (s: Settings) => invoke<void>('set_settings', { settings: s }),
};

// Session types
export interface SessionMessage {
  role: string;
  content: string;
  toolName?: string;
}

export interface SessionListItem {
  id: string;
  display_name: string;
  created_at: string;
  updated_at: string;
  message_count: number;
}

// Session commands (chat history persistence)
export const session = {
  save: (messages: SessionMessage[]) => invoke<string>('save_chat_session', { messages }),
  load: (sessionId?: string) => invoke<SessionMessage[] | null>('load_chat_session', { sessionId }),
  list: (dateFilter?: string) => invoke<SessionListItem[]>('list_chat_sessions', { dateFilter }),
  getLatest: () => invoke<SessionMessage[] | null>('get_latest_chat_session'),
};

// Terminal commands
export const terminal = {
  spawn: (cwd?: string) => invoke<string>('spawn_terminal', { cwd }),
  write: (sessionId: string, data: string) => invoke<void>('write_terminal', { sessionId, data }),
  resize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>('resize_terminal', { sessionId, cols, rows }),
  kill: (sessionId: string) => invoke<void>('kill_terminal', { sessionId }),
  list: () => invoke<string[]>('list_terminals'),
};

// Tool call/result types
export interface ToolCallPayload {
  name: string;
  args: string;
}

export interface ToolResultPayload {
  name: string;
  result: string;
}

export interface LlmStats {
  tokens_per_second?: number;
  total_tokens?: number;
  prompt_tokens?: number;
  completion_tokens?: number;
  total_ms?: number;
}

// Event listeners
export const events = {
  onLlmChunk: (callback: (chunk: { delta: string; done: boolean }) => void) =>
    listen<{ delta: string; done: boolean }>('llm:chunk', (e) => callback(e.payload)),

  onLlmThinking: (callback: (thinking: { delta: string; done: boolean }) => void) =>
    listen<{ delta: string; done: boolean }>('llm:thinking', (e) => callback(e.payload)),

  onToolCall: (callback: (toolCall: ToolCallPayload) => void) =>
    listen<ToolCallPayload>('llm:tool_call', (e) => callback(e.payload)),

  onToolResult: (callback: (toolResult: ToolResultPayload) => void) =>
    listen<ToolResultPayload>('llm:tool_result', (e) => callback(e.payload)),

  onLlmStatus: (callback: (status: { message: string; icon: string }) => void) =>
    listen<{ message: string; icon: string }>('llm:status', (e) => callback(e.payload)),

  onLlmStats: (callback: (stats: LlmStats) => void) =>
    listen<LlmStats>('llm:stats', (e) => callback(e.payload)),

  onLlmDebug: (callback: (message: string) => void) =>
    listen<string>('llm:debug', (e) => callback(e.payload)),

  onGhostUpdate: (callback: (update: GhostUpdate) => void) =>
    listen<GhostUpdate>('ghost:update', (e) => callback(e.payload)),

  onTerminalOutput: (callback: (data: { sessionId: string; data: string }) => void) =>
    listen<{ sessionId: string; data: string }>('terminal:output', (e) => callback(e.payload)),

  onFileChanged: (callback: (data: { path: string; kind: string }) => void) =>
    listen<{ path: string; kind: string }>('file:changed', (e) => callback(e.payload)),

  // Context engine events
  onContextIndexProgress: (callback: (progress: IndexProgress) => void) =>
    listen<IndexProgress>('context:index_progress', (e) => callback(e.payload)),

  onContextIndexComplete: (callback: (stats: IndexStats) => void) =>
    listen<IndexStats>('context:index_complete', (e) => callback(e.payload)),

  onContextFileChanged: (callback: (data: { path: string; action: string }) => void) =>
    listen<{ path: string; action: string }>('context:file_changed', (e) => callback(e.payload)),
};

// Context engine types
export interface IndexProgress {
  currentFile: string;
  filesDone: number;
  filesTotal: number;
  chunksCreated: number;
}

export interface IndexStats {
  filesIndexed: number;
  chunksCreated: number;
  totalTokens: number;
  durationMs: number;
}

export interface ContextChunk {
  id: string;
  filePath: string;
  startLine: number;
  endLine: number;
  content: string;
  language: string;
  chunkType: string;
  symbolName: string | null;
  tokenCount: number;
  score: number;
}

// Context engine commands
export const context = {
  init: (workspacePath: string, ollamaUrl?: string, embeddingModel?: string) =>
    invoke<void>('init_context_engine', { workspacePath, ollamaUrl, embeddingModel }),

  indexWorkspace: () => invoke<IndexStats>('index_workspace'),

  reindexFile: (filePath: string) => invoke<void>('reindex_file', { filePath }),

  invalidateFile: (filePath: string) => invoke<void>('invalidate_file', { filePath }),

  query: (query: string, maxTokens?: number) =>
    invoke<ContextChunk[]>('query_context', { query, maxTokens }),

  getFileContext: (filePaths: string[]) =>
    invoke<ContextChunk[]>('get_file_context', { filePaths }),
};

// Context watcher commands
export const contextWatcher = {
  watchWorkspace: (workspacePath: string) =>
    invoke<void>('watch_workspace_for_context', { workspacePath }),

  unwatchWorkspace: () => invoke<void>('unwatch_workspace_for_context'),

  isEnabled: () => invoke<boolean>('is_context_watching_enabled'),
};

// Structured Edit types
export interface EditHunk {
  id: string;
  filePath: string;
  startLine: number;
  endLine: number;
  original: string;
  replacement: string;
  description: string;
  status: 'pending' | 'accepted' | 'rejected' | 'applied';
}

export interface StructuredEditResponse {
  sessionId: string;
  hunks: EditHunk[];
}

export interface StructuredEditRequest {
  prompt: string;
  focusFiles?: string[];
  currentFile?: {
    path: string;
    content: string;
    cursorLine?: number;
  };
}

// Structured Edit commands (with hunks)
export const structuredEdit = {
  request: (request: StructuredEditRequest) =>
    invoke<StructuredEditResponse>('request_structured_edit', { request }),

  updateHunkStatus: (sessionId: string, hunkId: string, accepted: boolean) =>
    invoke<void>('update_hunk_status', { sessionId, hunkId, accepted }),

  applyAccepted: (sessionId: string) =>
    invoke<string[]>('apply_accepted_hunks', { sessionId }),

  cancel: (sessionId: string) =>
    invoke<void>('cancel_edit_session', { sessionId }),

  getSession: (sessionId: string) =>
    invoke<StructuredEditResponse | null>('get_edit_session', { sessionId }),
};

// Native dialogs
export const dialog = {
  openFile: async (filters?: { name: string; extensions: string[] }[]) => {
    const result = await open({
      multiple: false,
      directory: false,
      filters,
    });
    return result as string | null;
  },

  openFiles: async (filters?: { name: string; extensions: string[] }[]) => {
    const result = await open({
      multiple: true,
      directory: false,
      filters,
    });
    return result as string[] | null;
  },

  openFolder: async () => {
    const result = await open({
      multiple: false,
      directory: true,
    });
    return result as string | null;
  },

  saveFile: async (defaultPath?: string, filters?: { name: string; extensions: string[] }[]) => {
    const result = await save({
      defaultPath,
      filters,
    });
    return result as string | null;
  },
};
