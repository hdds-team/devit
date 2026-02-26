<script lang="ts">
  import Chat from './Chat.svelte';
  import Terminal from './Terminal.svelte';
  import Output from './Output.svelte';
  import Search from './Search.svelte';
  import type { Settings } from '../lib/tauri-ipc';

  interface Props {
    onGhostTrigger: (prompt: string) => Promise<string | null>;
    onFileSelect?: (path: string, line: number, column: number) => void;
    settings?: Settings | null;
    currentFile?: string | null;
    getEditorContent?: () => string | null;
  }

  let { onGhostTrigger, onFileSelect, settings = null, currentFile = null, getEditorContent }: Props = $props();

  let activeTab = $state<'chat' | 'terminal' | 'output' | 'search'>('chat');
  let terminalMounted = $state(false); // Lazy mount terminal
  let outputMounted = $state(false); // Lazy mount output
  let searchMounted = $state(false); // Lazy mount search
  let chatRef: Chat;
  let terminalRef: Terminal;
  let outputRef: Output;

  // Mount panels when first switched to
  $effect(() => {
    if (activeTab === 'terminal' && !terminalMounted) {
      terminalMounted = true;
    }
    if (activeTab === 'output' && !outputMounted) {
      outputMounted = true;
    }
    if (activeTab === 'search' && !searchMounted) {
      searchMounted = true;
    }
  });

  // Public methods to expose child refs
  export function clearChat() {
    chatRef?.clearMessages();
  }

  export function focusTerminal() {
    activeTab = 'terminal';
    setTimeout(() => terminalRef?.focus(), 50);
  }

  export function switchToChat() {
    activeTab = 'chat';
  }

  export function switchToTerminal() {
    activeTab = 'terminal';
  }

  export function switchToOutput() {
    activeTab = 'output';
  }

  export function switchToSearch() {
    activeTab = 'search';
  }

  export function clearOutput() {
    outputRef?.clear();
  }
</script>

<div class="bottom-panel">
  <div class="tabs">
    <button
      class="tab"
      class:active={activeTab === 'chat'}
      onclick={() => activeTab = 'chat'}
    >
      Chat
    </button>
    <button
      class="tab"
      class:active={activeTab === 'terminal'}
      onclick={() => { activeTab = 'terminal'; setTimeout(() => terminalRef?.focus(), 50); }}
    >
      Terminal
    </button>
    <button
      class="tab"
      class:active={activeTab === 'output'}
      onclick={() => activeTab = 'output'}
    >
      Output
    </button>
    <button
      class="tab"
      class:active={activeTab === 'search'}
      onclick={() => activeTab = 'search'}
    >
      Search
    </button>
    <div class="spacer"></div>
  </div>

  <div class="content">
    <div class="pane" class:active={activeTab === 'chat'}>
      <Chat bind:this={chatRef} {onGhostTrigger} {settings} {currentFile} {getEditorContent} />
    </div>
    {#if terminalMounted}
      <div class="pane" class:active={activeTab === 'terminal'}>
        <Terminal bind:this={terminalRef} />
      </div>
    {/if}
    {#if outputMounted}
      <div class="pane" class:active={activeTab === 'output'}>
        <Output bind:this={outputRef} />
      </div>
    {/if}
    {#if searchMounted}
      <div class="pane" class:active={activeTab === 'search'}>
        <Search {onFileSelect} />
      </div>
    {/if}
  </div>
</div>

<style>
  .bottom-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-secondary);
  }

  .tabs {
    display: flex;
    align-items: center;
    height: 32px;
    background: var(--bg-tertiary, #2d2d2d);
    border-bottom: 1px solid var(--border);
    padding: 0 8px;
    gap: 2px;
  }

  .tab {
    padding: 6px 12px;
    background: transparent;
    border: none;
    border-radius: 4px 4px 0 0;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tab:hover {
    color: var(--text-primary);
    background: var(--bg-hover, rgba(255, 255, 255, 0.05));
  }

  .tab.active {
    color: var(--text-primary);
    background: var(--bg-secondary);
    border-bottom: 2px solid var(--accent);
  }

  .spacer {
    flex: 1;
  }

  .content {
    flex: 1;
    overflow: hidden;
    position: relative;
  }

  .pane {
    position: absolute;
    inset: 0;
    display: none;
    overflow: hidden;
  }

  .pane.active {
    display: flex;
    flex-direction: column;
  }
</style>
