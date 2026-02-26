<script lang="ts">
  import type { Symbol } from '../lib/tauri-ipc';

  interface Props {
    symbols: Symbol[];
    onNavigate?: (line: number) => void;
  }

  let { symbols, onNavigate }: Props = $props();

  function getIcon(kind: string) {
    switch (kind) {
      case 'function': return 'ƒ';
      case 'class': return 'C';
      case 'interface': return 'I';
      case 'variable': return 'v';
      case 'constant': return 'c';
      case 'struct': return 'S';
      case 'enum': return 'E';
      case 'trait': return 'T';
      case 'impl': return 'i';
      case 'type': return 't';
      default: return '•';
    }
  }

  function handleClick(symbol: Symbol) {
    onNavigate?.(symbol.line);
  }
</script>

<div class="functions">
  <div class="header">
    <span>OUTLINE</span>
  </div>
  
  <div class="list">
    {#if symbols.length === 0}
      <div class="empty">No symbols</div>
    {:else}
      {#each symbols as symbol}
        <div class="symbol" onclick={() => handleClick(symbol)}>
          <span class="icon">{getIcon(symbol.kind)}</span>
          <span class="name truncate">{symbol.name}</span>
          <span class="line">:{symbol.line}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .functions {
    height: 100%;
    display: flex;
    flex-direction: column;
    font-size: var(--font-size-sm);
  }
  
  .header {
    padding: 8px 12px;
    font-weight: 600;
    font-size: 11px;
    letter-spacing: 0.5px;
    color: var(--text-secondary);
  }
  
  .list {
    flex: 1;
    overflow: auto;
  }
  
  .symbol {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    cursor: pointer;
  }
  
  .symbol:hover {
    background: var(--bg-hover);
  }
  
  .icon {
    width: 16px;
    text-align: center;
    color: var(--accent);
    font-family: var(--font-mono);
    font-weight: bold;
  }
  
  .name {
    flex: 1;
    font-family: var(--font-mono);
  }
  
  .line {
    color: var(--text-muted);
    font-family: var(--font-mono);
  }
  
  .empty {
    padding: 12px;
    color: var(--text-muted);
    text-align: center;
  }
</style>
