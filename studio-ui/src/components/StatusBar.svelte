<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';

  interface Props {
    file: string | null;
    message?: string | null;
    line?: number;
    column?: number;
    mode?: string;  // INS, OVR, etc.
    encoding?: string;
    lineEnding?: string;  // LF, CRLF
    aircpOnline?: boolean;
    agentsUp?: number;
    agentsTotal?: number;
    workflowPhase?: string | null;
  }

  let {
    file,
    message,
    line = 1,
    column = 1,
    mode = 'INS',
    encoding = 'UTF-8',
    lineEnding = 'LF',
    aircpOnline = false,
    agentsUp = 0,
    agentsTotal = 0,
    workflowPhase = null,
  }: Props = $props();

  // Context engine status
  interface ContextStatus {
    status: 'initializing' | 'indexing' | 'ready' | 'error';
    message: string;
    progress?: [number, number] | null;
  }

  let contextStatus = $state<ContextStatus | null>(null);
  let unlisten: UnlistenFn | null = null;

  // Status icons
  const statusIcons: Record<string, string> = {
    initializing: '\u23F3', // hourglass
    indexing: '\u2699',     // gear
    ready: '\u2713',        // checkmark
    error: '\u26A0',        // warning
  };

  // Status colors
  const statusColors: Record<string, string> = {
    initializing: '#ffc107', // yellow
    indexing: '#2196f3',     // blue
    ready: '#4caf50',        // green
    error: '#f44336',        // red
  };

  onMount(async () => {
    // Listen to context:status events
    unlisten = await listen<ContextStatus>('context:status', (event) => {
      contextStatus = event.payload;
    });
  });

  onDestroy(() => {
    if (unlisten) unlisten();
  });

  function getLanguage(path: string | null) {
    if (!path) return null;
    const ext = path.split('.').pop();
    const map: Record<string, string> = {
      rs: 'Rust',
      py: 'Python',
      js: 'JavaScript',
      ts: 'TypeScript',
      svelte: 'Svelte',
      json: 'JSON',
      md: 'Markdown',
      html: 'HTML',
      css: 'CSS',
      c: 'C',
      cpp: 'C++',
      h: 'C Header',
      go: 'Go',
      java: 'Java',
      sh: 'Shell',
      toml: 'TOML',
      yaml: 'YAML',
      yml: 'YAML',
    };
    return map[ext || ''] || ext?.toUpperCase();
  }

  function getFilename(path: string | null) {
    if (!path) return null;
    return path.split('/').pop();
  }

  function formatContextStatus(status: ContextStatus): string {
    if (status.status === 'indexing' && status.progress) {
      const [current, total] = status.progress;
      if (total > 0) {
        return `Indexing ${current}/${total}`;
      }
    }
    return status.message;
  }
</script>

<footer class="statusbar">
  <div class="left">
    {#if file}
      <span class="item filename">{getFilename(file)}</span>
    {/if}
    {#if message}
      <span class="message">{message}</span>
    {/if}
  </div>
  <div class="right">
    <!-- aIRCp status -->
    {#if aircpOnline}
      <span class="item aircp-status" title="aIRCp connected">
        <span class="aircp-dot online"></span>
        {agentsUp}/{agentsTotal} agents
      </span>
    {:else}
      <span class="item aircp-status offline" title="aIRCp disconnected">
        <span class="aircp-dot"></span>
        offline
      </span>
    {/if}
    {#if workflowPhase}
      <span class="item wf-badge" title="Active workflow">WF:{workflowPhase}</span>
    {/if}

    <!-- Context engine status -->
    {#if contextStatus && contextStatus.status !== 'ready'}
      <span
        class="item context-status"
        style="color: {statusColors[contextStatus.status]}"
        title={contextStatus.message}
      >
        <span class="status-icon">{statusIcons[contextStatus.status]}</span>
        {formatContextStatus(contextStatus)}
      </span>
    {:else if contextStatus?.status === 'ready'}
      <span
        class="item context-ready"
        title={contextStatus.message}
      >
        <span class="status-icon">{statusIcons.ready}</span>
        RAG
      </span>
    {/if}

    {#if file}
      <span class="item cursor">Ln {line}, Col {column}</span>
      <span class="item mode">{mode}</span>
      <span class="item">{lineEnding}</span>
      <span class="item">{encoding}</span>
      <span class="item lang">{getLanguage(file)}</span>
    {/if}
  </div>
</footer>

<style>
  .statusbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 22px;
    background: var(--bg-tertiary, #252526);
    border-top: 1px solid var(--border, #3c3c3c);
    color: var(--text-secondary, #969696);
    font-size: 11px;
    padding: 0 8px;
    user-select: none;
  }

  .left, .right {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .item {
    white-space: nowrap;
    padding: 0 4px;
  }

  .item:hover {
    background: var(--bg-hover, rgba(255, 255, 255, 0.05));
    border-radius: 2px;
  }

  .filename {
    color: var(--text-primary, #cccccc);
  }

  .cursor {
    min-width: 80px;
  }

  .mode {
    min-width: 28px;
    text-align: center;
    font-weight: 500;
  }

  .lang {
    color: var(--text-primary, #cccccc);
  }

  .message {
    background: var(--bg-hover, rgba(255, 255, 255, 0.1));
    padding: 1px 8px;
    border-radius: 3px;
    color: var(--text-primary);
  }

  .context-status {
    display: flex;
    align-items: center;
    gap: 4px;
    font-weight: 500;
  }

  .context-ready {
    display: flex;
    align-items: center;
    gap: 4px;
    color: #4caf50;
    font-weight: 500;
  }

  .status-icon {
    font-size: 10px;
  }

  .aircp-status {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .aircp-status.offline {
    color: #666;
  }

  .aircp-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #666;
  }

  .aircp-dot.online {
    background: #4caf50;
  }

  .wf-badge {
    background: var(--accent, #007acc);
    color: #fff;
    padding: 0 6px;
    border-radius: 3px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
  }
</style>
