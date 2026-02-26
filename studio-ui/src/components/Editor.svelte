<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { editor as editorApi, dialog, watcher, workspace, events, type Symbol } from '../lib/tauri-ipc';
  import { ghostCursor, startGhostEdit, isGhostActive } from '../lib/ghost-cursor';
  import { gitGutter, updateGitChanges } from '../lib/git-gutter';
  import { EditorView, basicSetup } from 'codemirror';
  import { EditorState } from '@codemirror/state';
  import { undo as undo_cmd, redo as redo_cmd } from '@codemirror/commands';
  import { openSearchPanel } from '@codemirror/search';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { javascript } from '@codemirror/lang-javascript';
  import { rust } from '@codemirror/lang-rust';
  import { python } from '@codemirror/lang-python';
  import { json } from '@codemirror/lang-json';
  import { markdown } from '@codemirror/lang-markdown';
  import { cpp } from '@codemirror/lang-cpp';
  import { html } from '@codemirror/lang-html';
  import { css } from '@codemirror/lang-css';
  import { StreamLanguage } from '@codemirror/language';
  import { shell } from '@codemirror/legacy-modes/mode/shell';

  interface CursorPosition {
    line: number;
    column: number;
  }

  interface Props {
    file: string | null;
    onSymbols: (symbols: Symbol[]) => void;
    onGhostRequest?: (prompt: string) => void;
    onSaveStatus?: (success: boolean, file: string | null) => void;
    onCursorChange?: (pos: CursorPosition) => void;
  }

  let { file, onSymbols, onGhostRequest, onSaveStatus, onCursorChange }: Props = $props();

  let editorEl: HTMLDivElement;
  let view: EditorView | null = null;
  let openTabs = $state<string[]>([]);
  let activeTab = $state<string | null>(null);
  let forceUpdate = $state(0);
  let lastFileFromProp = $state<string | null>(null); // Track last file prop to detect changes
  let modifiedFiles = $state<Set<string>>(new Set()); // Track unsaved files
  let originalContent = $state<Map<string, string>>(new Map()); // Original content for comparison
  let editorContent = $state<Map<string, string>>(new Map()); // Current content cache (for tab switching)

  // Expose view for ghost cursor integration
  export function getView(): EditorView | null {
    return view;
  }

  // Open a file (can be called externally to force open even if same file)
  export function openFile(path: string): void {
    loadFile(path);
  }

  // Reload current file from disk (bypass cache, useful when modified externally)
  export async function reload(): Promise<boolean> {
    if (!view || !activeTab) return false;
    try {
      // Use reloadFile to bypass backend cache
      const { content, language } = await editorApi.reloadFile(activeTab);

      // Update editor content without recreating the view
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: content
        }
      });

      // Refresh symbols
      const symbols = await editorApi.getSymbols(activeTab);
      onSymbols(symbols);

      return true;
    } catch (e) {
      console.error('Failed to reload:', e);
      return false;
    }
  }

  // Trigger ghost edit from external (e.g., chat)
  export async function triggerGhost(prompt: string): Promise<string | null> {
    if (!view || !activeTab) return null;
    return startGhostEdit(view, prompt, activeTab);
  }

  // Save current file
  export async function save(): Promise<boolean> {
    if (!view || !activeTab) return false;

    // Untitled files need Save As dialog
    if (activeTab.startsWith('Untitled-')) {
      return saveAs();
    }

    const content = view.state.doc.toString();
    try {
      await editorApi.saveFile(activeTab, content);
      // Update original content and clear modified state
      originalContent.set(activeTab, content);
      editorContent.set(activeTab, content); // Update cache too
      modifiedFiles.delete(activeTab);
      modifiedFiles = new Set(modifiedFiles);
      console.log('File saved:', activeTab);
      return true;
    } catch (e) {
      console.error('Failed to save:', e);
      return false;
    }
  }

  // Get current file path
  export function getCurrentFile(): string | null {
    return activeTab;
  }

  // Get current file content
  export function getCurrentContent(): string | null {
    if (!view) return null;
    return view.state.doc.toString();
  }

  // Navigate to a specific line
  export function goToLine(line: number): void {
    if (!view) return;
    const lineInfo = view.state.doc.line(Math.min(line, view.state.doc.lines));
    view.dispatch({
      selection: { anchor: lineInfo.from },
      scrollIntoView: true,
    });
    view.focus();
  }

  // Save As - prompt for new path
  export async function saveAs(): Promise<boolean> {
    if (!view) return false;

    const newPath = await dialog.saveFile(activeTab || undefined);
    if (!newPath) return false;

    const content = view.state.doc.toString();
    try {
      await editorApi.saveFile(newPath, content);
      // Clear old path state (including cached content)
      if (activeTab) {
        modifiedFiles.delete(activeTab);
        originalContent.delete(activeTab);
        editorContent.delete(activeTab);
      }
      // Update active tab to new path
      if (activeTab) {
        const idx = openTabs.indexOf(activeTab);
        if (idx !== -1) {
          openTabs = [...openTabs.slice(0, idx), newPath, ...openTabs.slice(idx + 1)];
        }
      } else {
        openTabs = [...openTabs, newPath];
      }
      activeTab = newPath;
      // Set as unmodified
      originalContent.set(newPath, content);
      modifiedFiles = new Set(modifiedFiles);
      return true;
    } catch (e) {
      console.error('Failed to save as:', e);
      return false;
    }
  }

  // Undo
  export function undo(): boolean {
    if (!view) return false;
    return undo_cmd(view);
  }

  // Redo
  export function redo(): boolean {
    if (!view) return false;
    return redo_cmd(view);
  }

  // Open find panel
  export function find(): boolean {
    if (!view) return false;
    openSearchPanel(view);
    return true;
  }

  // Close all tabs and reset editor state (for workspace change)
  export async function closeAllTabs(): Promise<void> {
    // Unwatch all files
    for (const tab of openTabs) {
      if (!tab.startsWith('Untitled-')) {
        try {
          await watcher.unwatch(tab);
        } catch (e) {
          console.warn('Failed to unwatch:', tab, e);
        }
      }
    }

    // Destroy editor view
    if (view) {
      view.destroy();
      view = null;
    }

    // Clear all state
    openTabs = [];
    activeTab = null;
    modifiedFiles = new Set();
    originalContent = new Map();
    editorContent = new Map();
    untitledCounter = 0;

    // Clear symbols
    onSymbols([]);
  }

  // Counter for untitled files
  let untitledCounter = 0;

  // Create new untitled file
  export function newFile(): void {
    // Save current editor content before switching
    if (view && activeTab) {
      editorContent.set(activeTab, view.state.doc.toString());
    }

    // Generate unique untitled name
    untitledCounter++;
    const untitledPath = `Untitled-${untitledCounter}`;

    // Add to tabs
    openTabs = [...openTabs, untitledPath];
    activeTab = untitledPath;

    // Store empty as original content
    originalContent.set(untitledPath, '');
    editorContent.set(untitledPath, '');

    // Create fresh editor
    if (view) {
      view.destroy();
    }

    const extensions = [
      basicSetup,
      oneDark,
      ghostCursor(),
      gitGutter(),
      EditorView.theme({
        '&': { height: '100%' },
        '.cm-scroller': { overflow: 'auto' },
      }),
      EditorView.updateListener.of((update) => {
        if (update.selectionSet || update.docChanged) {
          const pos = update.state.selection.main.head;
          const line = update.state.doc.lineAt(pos);
          onCursorChange?.({
            line: line.number,
            column: pos - line.from + 1
          });
        }
        // Track modifications for untitled files
        if (update.docChanged && activeTab?.startsWith('Untitled-')) {
          const currentContent = update.state.doc.toString();
          const original = originalContent.get(activeTab) || '';
          if (currentContent !== original) {
            modifiedFiles.add(activeTab);
            modifiedFiles = new Set(modifiedFiles);
          } else {
            modifiedFiles.delete(activeTab);
            modifiedFiles = new Set(modifiedFiles);
          }
        }
      }),
    ];

    view = new EditorView({
      state: EditorState.create({
        doc: '',
        extensions,
      }),
      parent: editorEl,
    });

    view.focus();
    onSymbols([]); // Clear symbols for new file
  }

  function getLanguageExtension(lang: string | null) {
    switch (lang) {
      case 'rust': return rust();
      case 'python': return python();
      case 'javascript':
      case 'javascriptreact':
      case 'typescript':
      case 'typescriptreact':
        return javascript({ typescript: lang?.includes('typescript') ?? false, jsx: lang?.includes('react') ?? false });
      case 'json': return json();
      case 'markdown': return markdown();
      case 'c':
      case 'cpp': return cpp();
      case 'html': return html();
      case 'css':
      case 'scss': return css();
      case 'shellscript': return StreamLanguage.define(shell);
      // Languages without CodeMirror support - fallback to no highlighting
      case 'go':
      case 'java':
      case 'ruby':
      case 'php':
      case 'sql':
      case 'yaml':
      case 'toml':
      case 'svelte':
      case 'vue':
      default: return [];
    }
  }
  
  async function loadFile(path: string) {
    // Save current editor content before switching
    if (view && activeTab) {
      editorContent.set(activeTab, view.state.doc.toString());
    }

    // Check if we have cached content (unsaved changes)
    let content: string;
    let language: string | null;

    if (editorContent.has(path)) {
      // Use cached content (preserves unsaved changes)
      content = editorContent.get(path)!;
      // Still need language info
      const fileInfo = await editorApi.openFile(path);
      language = fileInfo.language;
    } else {
      // Fresh load from disk
      const fileInfo = await editorApi.openFile(path);
      content = fileInfo.content;
      language = fileInfo.language;
      // Store as original content
      originalContent.set(path, content);
    }

    // Add to tabs if not already open
    if (!openTabs.includes(path)) {
      openTabs = [...openTabs, path];
      // Start watching this file for external changes
      watcher.watch(path).catch(console.error);
    }
    activeTab = path;

    // Create or update editor
    if (view) {
      view.destroy();
    }

    // Capture path for closure
    const currentPath = path;

    const extensions = [
      basicSetup,
      oneDark,
      getLanguageExtension(language),
      ghostCursor(),
      gitGutter(),
      EditorView.theme({
        '&': { height: '100%' },
        '.cm-scroller': { overflow: 'auto' },
      }),
      // Track cursor position and document changes
      EditorView.updateListener.of((update) => {
        if (update.selectionSet || update.docChanged) {
          const pos = update.state.selection.main.head;
          const line = update.state.doc.lineAt(pos);
          onCursorChange?.({
            line: line.number,
            column: pos - line.from + 1
          });
        }
        // Track modifications
        if (update.docChanged) {
          const currentContent = update.state.doc.toString();
          const original = originalContent.get(currentPath);
          if (currentContent !== original) {
            if (!modifiedFiles.has(currentPath)) {
              modifiedFiles.add(currentPath);
              modifiedFiles = new Set(modifiedFiles);
            }
          } else {
            if (modifiedFiles.has(currentPath)) {
              modifiedFiles.delete(currentPath);
              modifiedFiles = new Set(modifiedFiles);
            }
          }
        }
      }),
    ];

    view = new EditorView({
      state: EditorState.create({
        doc: content,
        extensions,
      }),
      parent: editorEl,
    });

    // Get symbols
    const symbols = await editorApi.getSymbols(path);
    onSymbols(symbols);

    // Load git diff for gutter
    loadGitDiff(path, view);
  }

  // Load git diff and update gutter
  async function loadGitDiff(path: string, editorView: EditorView | null) {
    if (!editorView) return;
    try {
      const changes = await workspace.getFileDiff(path);
      updateGitChanges(editorView, changes);
    } catch (e) {
      console.debug('Git diff not available:', e);
    }
  }
  
  function closeTab(path: string) {
    const idx = openTabs.indexOf(path);
    if (idx === -1) return;

    const wasActive = activeTab === path;

    // Stop watching this file
    watcher.unwatch(path).catch(console.error);

    // Clean up caches for this file
    editorContent.delete(path);
    originalContent.delete(path);
    modifiedFiles.delete(path);
    modifiedFiles = new Set(modifiedFiles);

    // Créer un nouveau tableau pour forcer la réactivité
    openTabs = openTabs.filter(t => t !== path);
    forceUpdate++;

    if (wasActive) {
      if (openTabs.length > 0) {
        activeTab = openTabs[openTabs.length - 1];
        loadFile(activeTab);
      } else {
        activeTab = null;
        if (view) {
          view.destroy();
          view = null;
        }
      }
    }
  }
  
  function getFileName(path: string) {
    return path.split('/').pop() || path;
  }
  
  $effect(() => {
    // Réagir uniquement aux changements du prop 'file'
    // untrack sur openTabs pour ne pas re-déclencher lors de fermeture d'onglet
    if (file && (file !== lastFileFromProp || !untrack(() => openTabs.includes(file)))) {
      lastFileFromProp = file;
      loadFile(file);
    }
  });

  // Keyboard shortcuts
  function handleKeydown(e: KeyboardEvent) {
    // Ctrl+S or Cmd+S to save
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault();
      save().then((success) => {
        onSaveStatus?.(success, activeTab);
      });
    }
    // F5 to reload from disk
    if (e.key === 'F5') {
      e.preventDefault();
      reload();
    }
  }

  let unsubscribeFileChanged: (() => void) | null = null;

  onMount(async () => {
    window.addEventListener('keydown', handleKeydown);

    // Listen for file changes from watcher
    unsubscribeFileChanged = await events.onFileChanged((data) => {
      // Auto-reload if the changed file is currently active
      if (data.path === activeTab && data.kind === 'modified') {
        console.log('File changed externally, reloading:', data.path);
        reload();
      }
    });
  });

  onDestroy(() => {
    window.removeEventListener('keydown', handleKeydown);
    unsubscribeFileChanged?.();

    // Unwatch all open files
    for (const tab of openTabs) {
      watcher.unwatch(tab).catch(console.error);
    }
  });
</script>

<div class="editor-container">
  {#key forceUpdate}
  <div
    class="tabs"
    role="tablist"
    onwheel={(e) => {
      e.currentTarget.scrollLeft += e.deltaY;
      e.preventDefault();
    }}
  >
    {#each openTabs as tab, index}
      {@const tabPath = tab}
      <div
        class="tab"
        class:active={tab === activeTab}
        class:modified={modifiedFiles.has(tabPath)}
        role="tab"
        tabindex="0"
        onclick={() => loadFile(tabPath)}
        onkeydown={(e) => e.key === 'Enter' && loadFile(tabPath)}
        onauxclick={(e) => {
          if (e.button === 1) {
            e.preventDefault();
            closeTab(tabPath);
          }
        }}
      >
        {#if modifiedFiles.has(tabPath)}
          <span class="modified-dot">●</span>
        {/if}
        <span class="tab-name truncate">{getFileName(tab)}</span>
        <button
          type="button"
          class="tab-close"
          onclick={(e) => {
            e.stopPropagation();
            closeTab(tabPath);
          }}
          aria-label="Close tab"
        >×</button>
      </div>
    {/each}
  </div>
  {/key}

  <!-- Editor -->
  <div class="editor" bind:this={editorEl}>
    {#if !activeTab}
      <div class="placeholder">
        <p>Open a file to start editing</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .editor-container {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--editor-bg);
  }
  
  .tabs {
    display: flex;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    min-height: 36px;
    position: relative;
    z-index: 10;
    scroll-behavior: smooth;
    scrollbar-width: none; /* Firefox */
  }

  .tabs::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  
  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border);
    cursor: pointer;
    font-size: var(--font-size-sm);
    max-width: 150px;
  }
  
  .tab.active {
    background: var(--editor-bg);
  }

  .tab.modified .tab-name {
    font-style: italic;
  }

  .modified-dot {
    color: #ff9800;
    font-size: 14px;
    text-shadow: 0 0 4px #ff9800;
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .tab:hover {
    background: var(--bg-hover);
  }

  .tab-name {
    flex: 1;
    pointer-events: none;
  }
  
  .tab-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 8px;
    font-size: 14px;
    line-height: 1;
    border-radius: 3px;
    flex-shrink: 0;
  }

  .tab-close:hover {
    color: var(--text-primary);
    background: rgba(255, 255, 255, 0.1);
  }
  
  .editor {
    flex: 1;
    overflow: hidden;
    position: relative;
    z-index: 1;
  }
  
  .placeholder {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }
</style>
