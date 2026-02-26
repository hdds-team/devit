<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Toolbar from './components/Toolbar.svelte';
  import ActionBar from './components/ActionBar.svelte';
  import ActivityBar from './components/ActivityBar.svelte';
  import Explorer from './components/Explorer.svelte';
  import Editor from './components/Editor.svelte';
  import Functions from './components/Functions.svelte';
  import TeamPanel from './components/TeamPanel.svelte';
  import BottomPanel from './components/BottomPanel.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import SettingsModal from './components/SettingsModal.svelte';
  import ProviderModal from './components/ProviderModal.svelte';
  import ModelSettingsModal from './components/ModelSettingsModal.svelte';
  import { dialog, settings, type Settings } from './lib/tauri-ipc';
  import { output } from './lib/stores';
  import { aircpStore } from './lib/aircp-store.svelte';

  let currentFile = $state<string | null>(null);
  let appSettings = $state<Settings | null>(null);

  // Load settings on mount
  onMount(async () => {
    output.info('System', 'devit-studio started');
    await loadSettings();
    output.info('System', 'Settings loaded');
    aircpStore.init();
  });

  onDestroy(() => {
    aircpStore.cleanup();
  });

  async function loadSettings() {
    try {
      appSettings = await settings.get();
    } catch (e) {
      console.error('Failed to load settings:', e);
      output.error('System', `Failed to load settings: ${e}`);
    }
  }

  async function handleSettingsClose() {
    showSettings = false;
    // Reload settings after modal closes
    await loadSettings();
  }

  function handleSettingsPreview(previewSettings: Settings) {
    // Live preview - update settings without saving
    appSettings = previewSettings;
  }
  let symbols = $state<any[]>([]);
  let editorRef: Editor;
  let explorerRef: Explorer;
  let bottomPanelRef: BottomPanel;
  let statusMessage = $state<string | null>(null);
  let cursorLine = $state(1);
  let cursorColumn = $state(1);

  // Modal states
  let showSettings = $state(false);
  let showProviderSelect = $state(false);
  let showModelSettings = $state(false);
  let currentProvider = $state('ollama');

  // Panel visibility
  let showExplorer = $state(true);
  let showBottomPanel = $state(true);
  let showOutline = $state(true);

  // Activity bar view
  let activeView = $state<string>('explorer');

  // Zoom level
  let zoomLevel = $state(100);

  // Resizable panels
  let bottomPanelHeight = $state(250);
  let leftPanelWidth = $state(220);
  let rightPanelWidth = $state(200);
  let resizingPanel = $state<'left' | 'right' | 'bottom' | null>(null);

  function startResizeLeft(e: MouseEvent) {
    resizingPanel = 'left';
    e.preventDefault();
  }

  function startResizeRight(e: MouseEvent) {
    resizingPanel = 'right';
    e.preventDefault();
  }

  function startResizeBottom(e: MouseEvent) {
    resizingPanel = 'bottom';
    e.preventDefault();
  }

  function handleMouseMove(e: MouseEvent) {
    if (!resizingPanel) return;

    if (resizingPanel === 'bottom') {
      const container = document.querySelector('.center') as HTMLElement;
      if (!container) return;
      const rect = container.getBoundingClientRect();
      const newHeight = rect.bottom - e.clientY;
      bottomPanelHeight = Math.max(100, Math.min(newHeight, rect.height - 100));
    } else if (resizingPanel === 'left') {
      const main = document.querySelector('.main') as HTMLElement;
      if (!main) return;
      const rect = main.getBoundingClientRect();
      const newWidth = e.clientX - rect.left;
      leftPanelWidth = Math.max(150, Math.min(newWidth, 400));
    } else if (resizingPanel === 'right') {
      const main = document.querySelector('.main') as HTMLElement;
      if (!main) return;
      const rect = main.getBoundingClientRect();
      const newWidth = rect.right - e.clientX;
      rightPanelWidth = Math.max(150, Math.min(newWidth, 400));
    }
  }

  function stopResize() {
    resizingPanel = null;
  }

  // Trigger ghost edit from Chat
  async function handleGhostTrigger(prompt: string): Promise<string | null> {
    if (!editorRef) return null;
    return editorRef.triggerGhost(prompt);
  }

  // Handle toolbar menu actions
  async function handleToolbarAction(action: string) {
    switch (action) {
      // File menu
      case 'save':
        if (editorRef) {
          const success = await editorRef.save();
          const file = editorRef.getCurrentFile();
          handleSaveStatus(success, file);
        }
        break;
      case 'save-as':
        if (editorRef) {
          const success = await editorRef.saveAs();
          if (success) {
            const file = editorRef.getCurrentFile();
            handleSaveStatus(success, file);
          }
        }
        break;
      case 'open-file':
        const filePath = await dialog.openFile();
        if (filePath) {
          currentFile = filePath;
          editorRef?.openFile(filePath);
        }
        break;
      case 'open-folder':
        if (explorerRef) {
          await explorerRef.openFolder();
        }
        break;
      case 'new-file':
        // Create empty untitled file in editor
        if (editorRef) {
          editorRef.newFile();
          currentFile = null;
          statusMessage = 'New file created';
          setTimeout(() => statusMessage = null, 2000);
        }
        break;

      // Edit menu
      case 'undo':
        editorRef?.undo();
        break;
      case 'redo':
        editorRef?.redo();
        break;
      case 'find':
        editorRef?.find();
        break;
      case 'find-in-files':
        // Open search panel
        if (showBottomPanel) {
          bottomPanelRef?.switchToSearch();
        } else {
          showBottomPanel = true;
          bottomPanelRef?.switchToSearch();
        }
        break;
      case 'split-editor':
        // TODO: Implement split view
        statusMessage = 'Split Editor - Coming soon!';
        setTimeout(() => statusMessage = null, 2000);
        break;
      case 'git-status':
        // TODO: Open git panel
        statusMessage = 'Git Status - Coming soon!';
        setTimeout(() => statusMessage = null, 2000);
        break;
      case 'reload':
        if (editorRef) {
          const success = await editorRef.reload();
          if (success) {
            statusMessage = 'File reloaded';
            setTimeout(() => statusMessage = null, 2000);
          }
        }
        break;

      // View menu
      case 'toggle-explorer':
        showExplorer = !showExplorer;
        activeView = showExplorer ? 'explorer' : '';
        break;
      case 'toggle-chat':
        if (showBottomPanel) {
          bottomPanelRef?.switchToChat();
        } else {
          showBottomPanel = true;
          bottomPanelRef?.switchToChat();
        }
        break;
      case 'toggle-terminal':
        if (showBottomPanel) {
          bottomPanelRef?.switchToTerminal();
        } else {
          showBottomPanel = true;
          bottomPanelRef?.switchToTerminal();
        }
        break;
      case 'zoom-in':
        zoomLevel = Math.min(150, zoomLevel + 10);
        document.documentElement.style.fontSize = `${zoomLevel}%`;
        break;
      case 'zoom-out':
        zoomLevel = Math.max(70, zoomLevel - 10);
        document.documentElement.style.fontSize = `${zoomLevel}%`;
        break;

      // Help menu
      case 'about':
        alert('devit-studio v0.1.0\nA local-first IDE with integrated LLM');
        break;
      case 'docs':
        window.open('https://github.com/anthropics/claude-code', '_blank');
        break;
      case 'shortcuts':
        alert(`Keyboard Shortcuts:

File:
  Ctrl+S     Save
  Ctrl+O     Open File

Edit:
  Ctrl+Z     Undo
  Ctrl+Y     Redo
  Ctrl+F     Find
  Ctrl+H     Replace

Editor:
  Tab        Accept ghost suggestion
  Escape     Reject ghost suggestion

Chat:
  Enter      Send message
  /ghost     Toggle ghost mode`);
        break;

      // Settings
      case 'settings':
        showSettings = true;
        break;

      // LLM menu
      case 'select-provider':
        showProviderSelect = true;
        break;
      case 'model-settings':
        showModelSettings = true;
        break;
      case 'clear-chat':
        bottomPanelRef?.clearChat();
        statusMessage = 'Chat cleared';
        setTimeout(() => statusMessage = null, 2000);
        break;

      default:
        // Handle breadcrumb clicks
        if (action.startsWith('breadcrumb:')) {
          // Reveal current file in explorer
          if (currentFile && explorerRef) {
            explorerRef.revealPath(currentFile);
          }
        } else {
          console.log('Unhandled action:', action);
        }
    }
  }

  // Handle save status feedback
  function handleSaveStatus(success: boolean, file: string | null) {
    if (success && file) {
      const fileName = file.split('/').pop();
      statusMessage = `Saved ${fileName}`;
      setTimeout(() => statusMessage = null, 2000);
    } else if (!success && file) {
      statusMessage = 'Save failed!';
      setTimeout(() => statusMessage = null, 3000);
    }
  }
</script>

<svelte:window onmousemove={handleMouseMove} onmouseup={stopResize} />

<div class="app" class:resizing={resizingPanel !== null}>
  <!-- Top toolbar (menu bar) -->
  <Toolbar onAction={handleToolbarAction} />

  <!-- Action bar (quick access buttons) -->
  <ActionBar onAction={handleToolbarAction} currentFile={currentFile} />

  <!-- Main content area -->
  <div class="main">
    <!-- Activity Bar -->
    <ActivityBar
      {activeView}
      agentsUp={aircpStore.agentsUp}
      onViewChange={(view) => {
        activeView = view;
        if (view === 'explorer') { showExplorer = true; showOutline = true; }
        else if (view === 'team') { showExplorer = false; }
        else if (view === '') { showExplorer = false; }
        else { showExplorer = view === 'explorer'; }
      }}
    />

    <!-- Left dock: Explorer -->
    {#if activeView === 'explorer'}
      <aside class="dock dock-left" style="width: {leftPanelWidth}px">
        <Explorer bind:this={explorerRef} onFileSelect={(path) => {
          currentFile = path;
          editorRef?.openFile(path);
        }} onWorkspaceChange={async (newPath) => {
          // Reset editor when workspace changes
          await editorRef?.closeAllTabs();
          currentFile = null;
          statusMessage = `Opened ${newPath.split('/').pop()}`;
          setTimeout(() => statusMessage = null, 3000);
        }} />
      </aside>
      <div class="resize-handle-v" onmousedown={startResizeLeft}></div>
    {/if}

    <!-- Center: Editor + Chat -->
    <main class="center">
      <div class="editor-area">
        <Editor
          bind:this={editorRef}
          file={currentFile}
          onSymbols={(s) => symbols = s}
          onSaveStatus={handleSaveStatus}
          onCursorChange={(pos) => { cursorLine = pos.line; cursorColumn = pos.column; }}
        />
      </div>
      {#if showBottomPanel}
        <!-- Resize handle -->
        <div class="resize-handle-h" onmousedown={startResizeBottom}></div>
        <div class="bottom-area" style="height: {bottomPanelHeight}px">
          <BottomPanel
            bind:this={bottomPanelRef}
            onGhostTrigger={handleGhostTrigger}
            settings={appSettings}
            currentFile={currentFile}
            getEditorContent={() => editorRef?.getCurrentContent() ?? null}
            onFileSelect={(path, line, column) => {
              currentFile = path;
              editorRef?.openFile(path);
              // Give editor time to load, then navigate to line
              setTimeout(() => editorRef?.goToLine(line), 100);
            }}
          />
        </div>
      {/if}
    </main>

    <!-- Right dock: Functions or Team -->
    {#if activeView === 'team'}
      <div class="resize-handle-v" onmousedown={startResizeRight}></div>
      <aside class="dock dock-right" style="width: {rightPanelWidth}px">
        <TeamPanel />
      </aside>
    {:else if showOutline}
      <div class="resize-handle-v" onmousedown={startResizeRight}></div>
      <aside class="dock dock-right" style="width: {rightPanelWidth}px">
        <Functions {symbols} onNavigate={(line) => editorRef?.goToLine(line)} />
      </aside>
    {/if}
  </div>

  <!-- Status bar -->
  <StatusBar
    file={currentFile}
    message={statusMessage}
    line={cursorLine}
    column={cursorColumn}
    aircpOnline={aircpStore.online}
    agentsUp={aircpStore.agentsUp}
    agentsTotal={aircpStore.agentsTotal}
    workflowPhase={aircpStore.workflow?.phase ?? null}
  />
</div>

<!-- Modals -->
<SettingsModal
  open={showSettings}
  onClose={handleSettingsClose}
  onPreview={handleSettingsPreview}
/>

<ProviderModal
  open={showProviderSelect}
  onClose={() => showProviderSelect = false}
  onSelect={(id) => {
    currentProvider = id;
    statusMessage = `Provider: ${id}`;
    setTimeout(() => statusMessage = null, 2000);
  }}
/>

<ModelSettingsModal
  open={showModelSettings}
  onClose={() => showModelSettings = false}
/>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg-primary);
  }

  .main {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .dock {
    background: var(--bg-secondary);
    border-color: var(--border);
  }

  .dock-left {
    border-right: 1px solid var(--border);
    flex-shrink: 0;
  }

  .dock-right {
    border-left: 1px solid var(--border);
    flex-shrink: 0;
  }

  .center {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0; /* Allow flex shrinking for proper scroll */
  }

  .editor-area {
    flex: 1;
    overflow: hidden;
    min-height: 0; /* Allow flex shrinking for proper scroll */
  }

  .bottom-area {
    border-top: 1px solid var(--border);
    overflow: hidden;
    max-height: 50vh; /* Never take more than half the viewport */
    flex-shrink: 0;
  }

  /* Horizontal resize handle (for bottom panel) */
  .resize-handle-h {
    height: 4px;
    background: transparent;
    cursor: ns-resize;
    position: relative;
    flex-shrink: 0;
  }

  .resize-handle-h:hover,
  .resizing .resize-handle-h {
    background: var(--accent);
  }

  .resize-handle-h::before {
    content: '';
    position: absolute;
    top: -3px;
    left: 0;
    right: 0;
    height: 10px;
  }

  /* Vertical resize handle (for left/right panels) */
  .resize-handle-v {
    width: 4px;
    background: transparent;
    cursor: ew-resize;
    position: relative;
    flex-shrink: 0;
  }

  .resize-handle-v:hover,
  .resizing .resize-handle-v {
    background: var(--accent);
  }

  .resize-handle-v::before {
    content: '';
    position: absolute;
    left: -3px;
    top: 0;
    bottom: 0;
    width: 10px;
  }

  .resizing {
    user-select: none;
  }
</style>
