/**
 * aIRCp Store -- Unified Svelte 5 rune store for Studio
 *
 * Agents presence + messages feed + workflow status via HDDS WebSocket.
 * Bootstrap from daemon HTTP, then live DDS updates.
 */
import { hdds } from './hdds-client.js';
import { TOPIC_PRESENCE, TOPIC_MESSAGES, TOPIC_WORKFLOWS, AGENTS, SYSTEM_BOTS, type AgentInfo } from './topics';
import { unwrapPayload } from './aircp-commands';
import { Cdr2Buffer, aircp } from './aircp_generated';

const DAEMON_URL = 'http://localhost:5555';
const MAX_MESSAGES = 50;
const ONLINE_THRESHOLD = 30;
const AWAY_THRESHOLD = 120;
const OPERATOR_ID = '@operator';

// --- Agent types ---
export interface Agent {
  id: string;
  model: string;
  role: string;
  color: string;
  health: 'online' | 'away' | 'dead';
  activity: string;
  lastSeen: number;
}

export interface ChatMessage {
  id: string;
  room: string;
  from: string;
  content: string;
  timestamp: Date;
}

export interface Workflow {
  feature: string;
  lead: string;
  phase: string;
  phases_done: number;
  phases_total: number;
}

// --- Reactive state ---
let agents = $state<Record<string, Agent>>({});
let messages = $state<ChatMessage[]>([]);
let workflow = $state<Workflow | null>(null);
let online = $state(false);

let _unsubs: Array<() => void> = [];
let _tickInterval: ReturnType<typeof setInterval>;
let _seenIds = new Set<string>();
let _stateUnsub: (() => void) | null = null;

// --- Derived ---
let agentList = $derived(
  Object.values(agents).sort((a, b) => {
    if (a.id === OPERATOR_ID) return -1;
    if (b.id === OPERATOR_ID) return 1;
    const order: Record<string, number> = { online: 0, away: 1, dead: 2 };
    return (order[a.health] ?? 2) - (order[b.health] ?? 2) || a.id.localeCompare(b.id);
  })
);

let agentsUp = $derived(Object.values(agents).filter(a => a.health === 'online').length);
let agentsTotal = $derived(Object.keys(agents).length);

// --- Presence handler ---
function onPresence(rawSample: any) {
  const sample = unwrapPayload(rawSample);
  const id = sample.agent_id || sample.from_id;
  if (!id) return;

  agents = {
    ...agents,
    [id]: {
      id,
      model: AGENTS[id]?.model || sample.model || '?',
      role: AGENTS[id]?.role || sample.role || '?',
      color: AGENTS[id]?.color || '#8b949e',
      health: sample.health || 'online',
      activity: sample.activity || sample.status || 'idle',
      lastSeen: Date.now(),
    },
  };
}

// --- Message handler ---
function onMessage(sample: any) {
  let msg = sample;
  if (sample._raw && typeof sample._raw === 'string') {
    try {
      const binary = atob(sample._raw);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      const buf = new Cdr2Buffer(bytes);
      msg = aircp.decodeMessage(buf);
    } catch { /* use raw */ }
  }

  const id = msg.id || crypto.randomUUID();
  if (_seenIds.has(id)) return;
  _seenIds.add(id);

  // Extract content from payload_json
  let content = '';
  const pj = msg.payload_json;
  if (pj) {
    try {
      const parsed = JSON.parse(typeof pj === 'string' ? pj : JSON.stringify(pj));
      content = parsed.content || parsed.text || parsed.message || String(pj);
    } catch {
      content = String(pj);
    }
  } else {
    content = msg.content || msg.message || msg.text || '';
  }

  // Timestamp
  let ts: Date;
  const raw = msg.timestamp_ns || msg.ts || msg.timestamp;
  if (typeof raw === 'bigint') {
    ts = new Date(Number(raw / 1000000n));
  } else if (typeof raw === 'number' && raw > 1e15) {
    ts = new Date(raw / 1e6);
  } else if (typeof raw === 'number' && raw > 1e12) {
    ts = new Date(raw);
  } else if (typeof raw === 'number') {
    ts = new Date(raw * 1000);
  } else {
    ts = new Date();
  }

  const parsed: ChatMessage = {
    id,
    room: msg.room || '#general',
    from: msg.from_id || msg.from || '?',
    content,
    timestamp: ts,
  };

  messages = [...messages, parsed].slice(-MAX_MESSAGES);
}

// --- Workflow handler ---
function onWorkflow(rawSample: any) {
  const sample = unwrapPayload(rawSample);
  if (sample.status === 'active' || sample.phase) {
    workflow = {
      feature: sample.feature || sample.name || '?',
      lead: sample.lead || '?',
      phase: sample.phase || '?',
      phases_done: sample.phases_done || sample.phase_index || 0,
      phases_total: sample.phases_total || 5,
    };
  } else if (sample.status === 'completed' || sample.status === 'aborted') {
    workflow = null;
  }
}

// --- Health decay tick ---
function tick() {
  const now = Date.now();
  let changed = false;
  const updated = { ...agents };
  for (const [id, agent] of Object.entries(updated)) {
    const elapsed = (now - agent.lastSeen) / 1000;
    let newHealth: Agent['health'] = 'online';
    if (elapsed > AWAY_THRESHOLD) newHealth = 'dead';
    else if (elapsed > ONLINE_THRESHOLD) newHealth = 'away';
    if (agent.health !== newHealth) {
      updated[id] = { ...agent, health: newHealth };
      changed = true;
    }
  }
  if (changed) agents = updated;
}

// --- Bootstrap from HTTP ---
async function _fetchAgents() {
  try {
    const res = await fetch(`${DAEMON_URL}/api/agents/presence`);
    if (!res.ok) return;
    const data = await res.json();
    const list = data.agents || data || [];
    for (const a of list) {
      const id = a.agent_id || a.id;
      if (!id) continue;
      agents = {
        ...agents,
        [id]: {
          id,
          model: AGENTS[id]?.model || a.model || '?',
          role: AGENTS[id]?.role || a.role || '?',
          color: AGENTS[id]?.color || '#8b949e',
          health: a.health || (a.seconds_since_heartbeat > 120 ? 'dead' : a.seconds_since_heartbeat > 30 ? 'away' : 'online'),
          activity: a.activity || a.status || 'idle',
          lastSeen: a.seconds_since_heartbeat ? Date.now() - (a.seconds_since_heartbeat * 1000) : 0,
        },
      };
    }
  } catch { /* daemon offline */ }
}

async function _fetchHistory() {
  try {
    const res = await fetch(`${DAEMON_URL}/api/history?room=%23general&limit=${MAX_MESSAGES}`);
    if (!res.ok) return;
    const data = await res.json();
    const rawMessages = data.messages || data.history || [];
    for (const m of rawMessages) {
      const id = m.id || crypto.randomUUID();
      if (_seenIds.has(id)) continue;
      _seenIds.add(id);

      const from = typeof m.from === 'object' ? m.from.id : (m.from || m.from_id || '?');
      let content = '';
      const pj = m.payload_json;
      if (pj) {
        try {
          const parsed = JSON.parse(typeof pj === 'string' ? pj : JSON.stringify(pj));
          content = parsed.content || parsed.text || parsed.message || String(pj);
        } catch { content = String(pj); }
      } else if (m.payload && typeof m.payload === 'object') {
        content = m.payload.content || m.payload.text || '';
      } else {
        content = m.content || m.message || m.text || '';
      }

      const raw = m.timestamp_ns || m.ts || m.timestamp;
      let ts: Date;
      if (typeof raw === 'number' && raw > 1e15) ts = new Date(raw / 1e6);
      else if (typeof raw === 'number' && raw > 1e12) ts = new Date(raw);
      else if (typeof raw === 'number') ts = new Date(raw * 1000);
      else ts = new Date(raw || Date.now());

      messages.push({ id, room: m.room || '#general', from, content, timestamp: ts });
    }
    messages = messages.sort((a, b) => a.timestamp.getTime() - b.timestamp.getTime()).slice(-MAX_MESSAGES);
  } catch { /* daemon offline */ }
}

async function _fetchWorkflow() {
  try {
    const res = await fetch(`${DAEMON_URL}/api/workflow/status`);
    if (!res.ok) return;
    const data = await res.json();
    if (data.status === 'active' && data.feature) {
      workflow = {
        feature: data.feature || '?',
        lead: data.lead || '?',
        phase: data.phase || '?',
        phases_done: data.phases_done || data.phase_index || 0,
        phases_total: data.phases_total || 5,
      };
    }
  } catch { /* daemon offline */ }
}

// --- Send message ---
function sendMessage(content: string, room: string = '#general') {
  const msg: aircp.Message = {
    id: crypto.randomUUID(),
    room,
    from_id: OPERATOR_ID,
    from_type: aircp.SenderType.USER,
    kind: aircp.MessageKind.CHAT,
    payload_json: JSON.stringify({ content }),
    timestamp_ns: BigInt(Date.now()) * 1000000n,
    protocol_version: '0.3.0',
    broadcast: true,
    to_agent_id: '',
    room_seq: 0n,
    project: '',
  };

  const buf = new Cdr2Buffer(new ArrayBuffer(8192));
  aircp.encodeMessage(msg, buf);
  const bytes = buf.toBytes();

  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }

  hdds.publish(TOPIC_MESSAGES(room), { _raw: btoa(binary) });
}

// --- Init / Cleanup ---
async function init() {
  cleanup();

  // Seed known agents as dead
  for (const [id, info] of Object.entries(AGENTS)) {
    if (!agents[id]) {
      agents = {
        ...agents,
        [id]: { id, ...info, health: 'dead' as const, activity: 'idle', lastSeen: 0 },
      };
    }
  }

  // Track connection state
  _stateUnsub = hdds.on('state', (s: string) => {
    online = s === 'connected';
  });

  // Connect WebSocket
  hdds.connect();

  // Bootstrap from HTTP (non-blocking)
  await Promise.all([_fetchAgents(), _fetchHistory(), _fetchWorkflow()]);

  // Live DDS subscriptions
  _unsubs.push(hdds.subscribe(TOPIC_PRESENCE, onPresence));
  _unsubs.push(hdds.subscribe(TOPIC_MESSAGES('#general'), onMessage, {
    reliability: 'reliable',
    history_depth: 50,
  }));
  _unsubs.push(hdds.subscribe('aircp/workflows', onWorkflow));

  // Health decay tick
  _tickInterval = setInterval(tick, 10000);
}

function cleanup() {
  _unsubs.forEach(fn => fn());
  _unsubs = [];
  _stateUnsub?.();
  _stateUnsub = null;
  clearInterval(_tickInterval);
  hdds.disconnect();
}

function getAgentColor(from: string): string {
  return AGENTS[from]?.color || '#8b949e';
}

function isSystem(from: string): boolean {
  return SYSTEM_BOTS.has(from);
}

export const aircpStore = {
  get agents() { return agents; },
  get agentList() { return agentList; },
  get messages() { return messages; },
  get workflow() { return workflow; },
  get online() { return online; },
  get agentsUp() { return agentsUp; },
  get agentsTotal() { return agentsTotal; },
  sendMessage,
  getAgentColor,
  isSystem,
  init,
  cleanup,
};
