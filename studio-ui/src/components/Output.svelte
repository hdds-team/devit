<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { outputLogs, clearOutputLogs, type OutputLog } from '../lib/stores';

  let logs = $state<OutputLog[]>([]);
  let container: HTMLDivElement;
  let autoScroll = $state(true);
  let unsubscribe: () => void;

  onMount(() => {
    unsubscribe = outputLogs.subscribe(value => {
      logs = value;
      if (autoScroll && container) {
        requestAnimationFrame(() => {
          container.scrollTop = container.scrollHeight;
        });
      }
    });
  });

  onDestroy(() => {
    unsubscribe?.();
  });

  function getIcon(level: OutputLog['level']): string {
    switch (level) {
      case 'error': return '❌';
      case 'warning': return '⚠️';
      case 'info': return 'ℹ️';
      case 'debug': return '🔍';
      default: return '•';
    }
  }

  function getLevelClass(level: OutputLog['level']): string {
    return `level-${level}`;
  }

  function formatTime(date: Date): string {
    return date.toLocaleTimeString('fr-FR', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit'
    });
  }

  export function clear() {
    clearOutputLogs();
  }
</script>

<div class="output">
  <div class="toolbar">
    <span class="title">Output</span>
    <div class="actions">
      <label class="auto-scroll">
        <input type="checkbox" bind:checked={autoScroll} />
        Auto-scroll
      </label>
      <button class="clear-btn" onclick={() => clear()} title="Clear output">
        🗑️
      </button>
    </div>
  </div>

  <div class="logs" bind:this={container}>
    {#if logs.length === 0}
      <div class="empty">No output yet</div>
    {:else}
      {#each logs as log}
        <div class="log-entry {getLevelClass(log.level)}">
          <span class="time">{formatTime(log.timestamp)}</span>
          <span class="icon">{getIcon(log.level)}</span>
          <span class="source">[{log.source}]</span>
          <span class="message">{log.message}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .output {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-primary);
  }

  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 12px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border);
    height: 28px;
  }

  .title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .auto-scroll {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .auto-scroll input {
    width: 12px;
    height: 12px;
  }

  .clear-btn {
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 3px;
  }

  .clear-btn:hover {
    background: var(--bg-hover);
  }

  .logs {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 12px;
    line-height: 1.6;
  }

  .empty {
    color: var(--text-muted);
    font-style: italic;
    padding: 20px;
    text-align: center;
  }

  .log-entry {
    display: flex;
    gap: 8px;
    padding: 2px 0;
    border-bottom: 1px solid var(--border-subtle, rgba(255,255,255,0.05));
  }

  .log-entry:last-child {
    border-bottom: none;
  }

  .time {
    color: var(--text-muted);
    font-size: 10px;
    min-width: 60px;
  }

  .icon {
    font-size: 11px;
  }

  .source {
    color: var(--text-secondary);
    min-width: 60px;
  }

  .message {
    color: var(--text-primary);
    flex: 1;
    word-break: break-word;
  }

  /* Level-specific colors */
  .level-error .message {
    color: #f87171;
  }

  .level-error .source {
    color: #f87171;
  }

  .level-warning .message {
    color: #fbbf24;
  }

  .level-warning .source {
    color: #fbbf24;
  }

  .level-info .message {
    color: var(--text-primary);
  }

  .level-debug .message {
    color: var(--text-muted);
  }

  .level-debug .source {
    color: var(--text-muted);
  }
</style>
