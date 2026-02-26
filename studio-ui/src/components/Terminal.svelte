<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal as XTerm } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { terminal, events } from '../lib/tauri-ipc';
  import '@xterm/xterm/css/xterm.css';

  interface TerminalTab {
    id: string;
    name: string;
    term: XTerm;
    fitAddon: FitAddon;
    sessionId: string | null;
  }

  let containerRef: HTMLDivElement;
  let tabs = $state<TerminalTab[]>([]);
  let activeTabId = $state<string | null>(null);
  let unsubscribe: (() => void) | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let tabCounter = $state(0);

  // Get active tab
  let activeTab = $derived(tabs.find(t => t.id === activeTabId) || null);

  onMount(async () => {
    // Listen for terminal output
    unsubscribe = await events.onTerminalOutput((payload) => {
      const tab = tabs.find(t => t.sessionId === payload.sessionId);
      if (tab) {
        tab.term.write(payload.data);
      }
    });

    // Create first terminal tab
    await createTab();

    // Setup resize observer
    resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry.contentRect.width > 0 && entry.contentRect.height > 0) {
        fitActiveTerminal();
      }
    });
    resizeObserver.observe(containerRef);
  });

  onDestroy(() => {
    if (resizeObserver) {
      resizeObserver.disconnect();
    }
    if (unsubscribe) {
      unsubscribe();
    }
    // Kill all sessions and dispose terminals
    for (const tab of tabs) {
      if (tab.sessionId) {
        terminal.kill(tab.sessionId).catch(console.error);
      }
      tab.term.dispose();
    }
  });

  async function createTab() {
    tabCounter++;
    const id = crypto.randomUUID();

    // Create xterm instance
    const term = new XTerm({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', Consolas, monospace",
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
        cursor: '#d4d4d4',
        cursorAccent: '#1e1e1e',
        selectionBackground: '#264f78',
        black: '#1e1e1e',
        red: '#f14c4c',
        green: '#4ec9b0',
        yellow: '#dcdcaa',
        blue: '#569cd6',
        magenta: '#c586c0',
        cyan: '#9cdcfe',
        white: '#d4d4d4',
        brightBlack: '#808080',
        brightRed: '#f14c4c',
        brightGreen: '#4ec9b0',
        brightYellow: '#dcdcaa',
        brightBlue: '#569cd6',
        brightMagenta: '#c586c0',
        brightCyan: '#9cdcfe',
        brightWhite: '#ffffff',
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    const tab: TerminalTab = {
      id,
      name: `Terminal ${tabCounter}`,
      term,
      fitAddon,
      sessionId: null,
    };

    // Add tab first so it's in the DOM
    tabs = [...tabs, tab];
    activeTabId = id;

    // Wait for DOM update, then open terminal
    await new Promise(r => setTimeout(r, 0));

    const terminalEl = containerRef.querySelector(`[data-tab-id="${id}"]`);
    if (terminalEl) {
      term.open(terminalEl as HTMLElement);
      fitAddon.fit();

      // Handle input
      term.onData((data: string) => {
        if (tab.sessionId) {
          terminal.write(tab.sessionId, data).catch(console.error);
        }
      });

      // Spawn session
      try {
        tab.sessionId = await terminal.spawn();
        term.writeln('Terminal session started.');
        // Trigger reactivity
        tabs = [...tabs];
      } catch (e) {
        term.writeln(`\x1b[31mFailed to start terminal: ${e}\x1b[0m`);
      }

      // Resize to sync with backend
      if (tab.sessionId) {
        terminal.resize(tab.sessionId, term.cols, term.rows).catch(() => {});
      }

      // Focus the terminal
      term.focus();
    }
  }

  async function closeTab(tabId: string) {
    const tabIndex = tabs.findIndex(t => t.id === tabId);
    if (tabIndex === -1) return;

    const tab = tabs[tabIndex];

    // Kill session and dispose terminal
    if (tab.sessionId) {
      await terminal.kill(tab.sessionId).catch(console.error);
    }
    tab.term.dispose();

    // Remove from list
    tabs = tabs.filter(t => t.id !== tabId);

    // Switch to another tab if this was active
    if (activeTabId === tabId) {
      if (tabs.length > 0) {
        const newIndex = Math.min(tabIndex, tabs.length - 1);
        activeTabId = tabs[newIndex].id;
        // Refit the new active terminal
        await new Promise(r => setTimeout(r, 0));
        fitActiveTerminal();
      } else {
        activeTabId = null;
      }
    }
  }

  function switchTab(tabId: string) {
    if (activeTabId === tabId) return;
    activeTabId = tabId;
    // Refit when switching tabs
    setTimeout(() => fitActiveTerminal(), 0);
  }

  function fitActiveTerminal() {
    if (activeTab) {
      activeTab.fitAddon.fit();
      if (activeTab.sessionId) {
        terminal.resize(activeTab.sessionId, activeTab.term.cols, activeTab.term.rows).catch(() => {});
      }
    }
  }

  // Public methods
  export function focus() {
    activeTab?.term.focus();
  }

  export function clear() {
    activeTab?.term.clear();
  }

  export async function newTerminal() {
    await createTab();
  }
</script>

<div class="terminal-wrapper">
  <!-- Tab bar -->
  <div class="tab-bar">
    <div class="tabs">
      {#each tabs as tab}
        <div
          class="tab"
          class:active={tab.id === activeTabId}
          onclick={() => switchTab(tab.id)}
          role="tab"
          tabindex="0"
          onkeydown={(e) => e.key === 'Enter' && switchTab(tab.id)}
        >
          <span class="tab-icon">💻</span>
          <span class="tab-name">{tab.name}</span>
          <button
            class="tab-close"
            onclick={(e) => { e.stopPropagation(); closeTab(tab.id); }}
            title="Close terminal"
          >×</button>
        </div>
      {/each}
    </div>
    <button class="new-tab" onclick={createTab} title="New Terminal">+</button>
  </div>

  <!-- Terminal containers -->
  <div class="terminal-container" bind:this={containerRef}>
    {#each tabs as tab}
      <div
        class="terminal-pane"
        class:active={tab.id === activeTabId}
        data-tab-id={tab.id}
        onclick={() => tab.term.focus()}
        role="application"
      ></div>
    {/each}
    {#if tabs.length === 0}
      <div class="empty">
        <button onclick={createTab}>New Terminal</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .terminal-wrapper {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #1e1e1e;
  }

  .tab-bar {
    display: flex;
    align-items: center;
    background: var(--bg-tertiary, #252526);
    border-bottom: 1px solid var(--border, #3c3c3c);
    min-height: 32px;
  }

  .tabs {
    display: flex;
    flex: 1;
    overflow-x: auto;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: transparent;
    border: none;
    color: var(--text-muted, #808080);
    font-size: 12px;
    cursor: pointer;
    border-right: 1px solid var(--border, #3c3c3c);
    white-space: nowrap;
  }

  .tab:hover {
    background: var(--bg-hover, #2a2a2a);
    color: var(--text-secondary, #cccccc);
  }

  .tab.active {
    background: #1e1e1e;
    color: var(--text-primary, #ffffff);
    border-bottom: 2px solid var(--accent, #4a9eff);
    margin-bottom: -1px;
  }

  .tab-icon {
    font-size: 12px;
  }

  .tab-name {
    max-width: 100px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tab-close {
    background: transparent;
    border: none;
    color: var(--text-muted, #808080);
    padding: 0 4px;
    font-size: 14px;
    cursor: pointer;
    border-radius: 3px;
    line-height: 1;
  }

  .tab-close:hover {
    background: rgba(255, 255, 255, 0.1);
    color: var(--text-primary, #ffffff);
  }

  .new-tab {
    background: transparent;
    border: none;
    color: var(--text-muted, #808080);
    font-size: 18px;
    padding: 4px 12px;
    cursor: pointer;
  }

  .new-tab:hover {
    color: var(--text-primary, #ffffff);
    background: var(--bg-hover, #2a2a2a);
  }

  .terminal-container {
    flex: 1;
    position: relative;
    overflow: hidden;
  }

  .terminal-pane {
    position: absolute;
    inset: 0;
    display: none;
  }

  .terminal-pane.active {
    display: block;
  }

  .terminal-pane :global(.xterm) {
    height: 100%;
    padding: 4px;
  }

  .terminal-pane :global(.xterm-screen) {
    height: 100%;
  }

  .terminal-pane :global(.xterm-viewport) {
    overflow-y: auto !important;
  }

  /* Ensure terminal can receive focus */
  .terminal-pane :global(.xterm textarea) {
    position: absolute !important;
  }

  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
  }

  .empty button {
    background: var(--accent, #4a9eff);
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
  }

  .empty button:hover {
    opacity: 0.9;
  }
</style>
