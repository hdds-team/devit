/**
 * Shared Svelte stores for devit-studio
 */
import { writable } from 'svelte/store';

// Current selected model (synced between Chat and ModelSettingsModal)
export const selectedModelStore = writable<string>('');

// Current selected provider
export const selectedProviderStore = writable<string>('ollama');

// Output logs for the Output panel
export interface OutputLog {
  timestamp: Date;
  level: 'error' | 'warning' | 'info' | 'debug';
  source: string;  // e.g., 'LSP', 'Build', 'System'
  message: string;
}

export const outputLogs = writable<OutputLog[]>([]);

// Helper functions to add logs
export function addOutputLog(level: OutputLog['level'], source: string, message: string) {
  outputLogs.update(logs => [
    ...logs,
    { timestamp: new Date(), level, source, message }
  ]);
}

export function clearOutputLogs() {
  outputLogs.set([]);
}

// Convenience functions
export const output = {
  error: (source: string, message: string) => addOutputLog('error', source, message),
  warning: (source: string, message: string) => addOutputLog('warning', source, message),
  info: (source: string, message: string) => addOutputLog('info', source, message),
  debug: (source: string, message: string) => addOutputLog('debug', source, message),
  clear: clearOutputLogs,
};
