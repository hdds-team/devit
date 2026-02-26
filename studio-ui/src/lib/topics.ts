/**
 * aIRCp Topic Map -- DDS topics used by Studio
 */

// Chat messages per room (bidirectional)
export const TOPIC_MESSAGES = (room: string): string => `aircp/${room.replace(/^#/, '')}`;

// Agent presence / heartbeats (subscribe only)
export const TOPIC_PRESENCE: string = 'aircp/presence';

// Task updates (subscribe only)
export const TOPIC_TASKS: string = 'aircp/tasks';

// Review updates (subscribe only)
export const TOPIC_REVIEWS: string = 'aircp/reviews';

// Workflow state changes (subscribe only)
export const TOPIC_WORKFLOWS: string = 'aircp/workflows';

// Mode changes (subscribe only)
export const TOPIC_MODE: string = 'aircp/mode';

// Commands from Studio -> daemon (publish only)
export const TOPIC_COMMANDS: string = 'aircp/commands';

// Rooms
export const DEFAULT_ROOMS: string[] = ['#general', '#brainstorm'];

// Agent definitions
export interface AgentInfo {
  model: string;
  role: string;
  color: string;
}

export const AGENTS: Record<string, AgentInfo> = {
  '@alpha':    { model: 'Claude Opus 4',   role: 'Lead dev',        color: '#f47067' },
  '@beta':     { model: 'Claude Opus 3',   role: 'QA / Review',     color: '#dcbdfb' },
  '@sonnet':   { model: 'Claude Sonnet 4', role: 'Analyse',         color: '#6cb6ff' },
  '@haiku':    { model: 'Claude Haiku',    role: 'Triage rapide',   color: '#8ddb8c' },
  '@mascotte': { model: 'Qwen3 (local)',   role: 'Assistant local', color: '#f69d50' },
  '@theta':    { model: 'LMStudio',        role: 'Assistant local', color: '#c084fc' },
  '@codex':    { model: 'GPT-5',           role: 'Code review',     color: '#e2c541' },
  '@operator': { model: 'Human',           role: 'Orchestrator',    color: '#57b8ff' },
};

// System bots (not shown as agents)
export const SYSTEM_BOTS: Set<string> = new Set([
  '@system', '@workflow', '@idea', '@review', '@taskman', '@watchdog', '@tips', '@brainstorm'
]);
