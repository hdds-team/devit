<script lang="ts">
  import { onMount } from 'svelte';
  import { workspace, dialog, type FileEntry } from '../lib/tauri-ipc';

  interface Props {
    onFileSelect: (path: string) => void;
    onWorkspaceChange?: (newPath: string) => void;
  }

  let { onFileSelect, onWorkspaceChange }: Props = $props();

  let tree = $state<FileEntry | null>(null);
  let expanded = $state<Set<string>>(new Set());
  let loading = $state<Set<string>>(new Set());

  // Git status
  interface GitStatus {
    branch: string | null;
    modified: string[];
    staged: string[];
    untracked: string[];
  }
  let gitStatus = $state<GitStatus | null>(null);
  let gitBranch = $derived(gitStatus?.branch || null);

  // Create lookup sets for fast status checking
  let modifiedSet = $derived(new Set(gitStatus?.modified || []));
  let stagedSet = $derived(new Set(gitStatus?.staged || []));
  let untrackedSet = $derived(new Set(gitStatus?.untracked || []));

  // Get git status indicator for a file path
  function getGitIndicator(filePath: string): { char: string; color: string } | null {
    // Get relative path from workspace root
    const rootPath = tree?.path || '';
    const relativePath = filePath.startsWith(rootPath)
      ? filePath.slice(rootPath.length + 1)
      : filePath;

    if (stagedSet.has(relativePath)) {
      return { char: 'A', color: '#4ec9b0' }; // green - staged/added
    }
    if (modifiedSet.has(relativePath)) {
      return { char: 'M', color: '#e5c07b' }; // yellow - modified
    }
    if (untrackedSet.has(relativePath)) {
      return { char: '?', color: '#abb2bf' }; // gray - untracked
    }
    return null;
  }

  // Load workspace on mount (restore last opened folder)
  onMount(async () => {
    try {
      const ws = await workspace.getWorkspace();
      if (ws) {
        tree = ws;
        expanded = new Set([ws.path]);
        await loadGitStatus();
      }
    } catch (e) {
      console.error('Failed to load workspace:', e);
    }
  });

  // Load git status
  async function loadGitStatus() {
    try {
      gitStatus = await workspace.getGitStatus();
    } catch (e) {
      console.error('Failed to load git status:', e);
      gitStatus = null;
    }
  }

  // Exposed for external calls (menu, etc.)
  export async function openFolder(): Promise<boolean> {
    const path = await dialog.openFolder();
    if (!path) return false;
    tree = await workspace.openFolder(path);
    expanded = new Set([path]);
    await loadGitStatus();
    // Notify parent that workspace changed
    onWorkspaceChange?.(path);
    return true;
  }

  // Refresh git status (exposed for external calls)
  export async function refreshGitStatus(): Promise<void> {
    await loadGitStatus();
  }

  // Refresh entire file tree
  export async function refreshTree(): Promise<void> {
    if (!tree) return;
    const rootPath = tree.path;
    try {
      tree = await workspace.openFolder(rootPath);
      // Re-expand previously expanded directories
      const oldExpanded = expanded;
      expanded = new Set([rootPath]);
      // Reload expanded directories
      for (const path of oldExpanded) {
        if (path !== rootPath) {
          expanded = new Set([...expanded, path]);
        }
      }
      await loadGitStatus();
    } catch (e) {
      console.error('Failed to refresh tree:', e);
    }
  }

  // Find and update an entry in the tree
  function updateEntry(root: FileEntry, path: string, newEntry: FileEntry): FileEntry {
    if (root.path === path) {
      return newEntry;
    }
    if (root.children) {
      return {
        ...root,
        children: root.children.map(child => updateEntry(child, path, newEntry))
      };
    }
    return root;
  }

  async function toggle(entry: FileEntry) {
    if (expanded.has(entry.path)) {
      // Collapse
      expanded = new Set([...expanded].filter(p => p !== entry.path));
    } else {
      // Expand - load children if not already loaded
      if (entry.is_dir && (!entry.children || entry.children.length === 0 ||
          entry.children.every(c => c.is_dir && !c.children))) {
        // Load children from backend
        loading = new Set([...loading, entry.path]);
        try {
          const loaded = await workspace.listFiles(entry.path, 1);
          if (tree) {
            tree = updateEntry(tree, entry.path, loaded);
          }
        } catch (e) {
          console.error('Failed to load directory:', e);
        }
        loading = new Set([...loading].filter(p => p !== entry.path));
      }
      expanded = new Set([...expanded, entry.path]);
    }
  }

  function select(entry: FileEntry) {
    if (entry.is_dir) {
      toggle(entry);
    } else {
      selectedPath = entry.path;
      onFileSelect(entry.path);
    }
  }

  // Track selected file for highlighting
  let selectedPath = $state<string | null>(null);

  // Reveal a file in the explorer (expand all parent directories)
  export async function revealPath(path: string): Promise<void> {
    if (!tree) return;

    const rootPath = tree.path;
    if (!path.startsWith(rootPath)) return;

    // Build list of directories to expand
    const relativePath = path.slice(rootPath.length + 1);
    const parts = relativePath.split('/');
    let currentPath = rootPath;
    const toExpand: string[] = [rootPath];

    // Expand all parent directories
    for (let i = 0; i < parts.length - 1; i++) {
      currentPath = currentPath + '/' + parts[i];
      toExpand.push(currentPath);
    }

    // Load and expand each directory
    for (const dir of toExpand) {
      if (!expanded.has(dir)) {
        // Load children if needed
        if (tree) {
          loading = new Set([...loading, dir]);
          try {
            const loaded = await workspace.listFiles(dir, 1);
            tree = updateEntry(tree, dir, loaded);
          } catch (e) {
            console.error('Failed to load directory:', dir, e);
          }
          loading = new Set([...loading].filter(p => p !== dir));
        }
        expanded = new Set([...expanded, dir]);
      }
    }

    // Set selected path for highlighting
    selectedPath = path;

    // Scroll into view if element exists
    setTimeout(() => {
      const el = document.querySelector(`[data-path="${CSS.escape(path)}"]`);
      el?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }, 100);
  }
</script>

<div class="explorer">
  <div class="header">
    <div class="header-left">
      <span>EXPLORER</span>
      {#if gitBranch}
        <span class="git-branch" title="Current branch">
          <span class="branch-icon">⎇</span>
          {gitBranch}
        </span>
      {/if}
    </div>
    <div class="header-actions">
      <button onclick={refreshTree} title="Refresh Files">🔄</button>
      <button onclick={openFolder} title="Open Folder">📁</button>
    </div>
  </div>
  
  {#if tree}
    <div class="tree">
      {#snippet renderEntry(entry: FileEntry, depth: number)}
        {@const gitIndicator = getGitIndicator(entry.path)}
        <div
          class="entry"
          class:dir={entry.is_dir}
          class:loading={loading.has(entry.path)}
          class:selected={selectedPath === entry.path}
          class:git-modified={gitIndicator?.char === 'M'}
          class:git-staged={gitIndicator?.char === 'A'}
          class:git-untracked={gitIndicator?.char === '?'}
          style="padding-left: {depth * 16 + 8}px"
          data-path={entry.path}
          onclick={() => select(entry)}
        >
          <span class="icon">
            {#if loading.has(entry.path)}
              ⏳
            {:else if entry.is_dir}
              {expanded.has(entry.path) ? '📂' : '📁'}
            {:else}
              📄
            {/if}
          </span>
          <span class="name truncate">{entry.name}</span>
          {#if gitIndicator}
            <span class="git-indicator" style="color: {gitIndicator.color}">
              {gitIndicator.char}
            </span>
          {/if}
        </div>
        {#if entry.is_dir && entry.children && expanded.has(entry.path)}
          {#each entry.children as child}
            {@render renderEntry(child, depth + 1)}
          {/each}
        {/if}
      {/snippet}
      
      {@render renderEntry(tree, 0)}
    </div>
  {:else}
    <div class="empty">
      <button onclick={openFolder}>Open Folder</button>
    </div>
  {/if}
</div>

<style>
  .explorer {
    height: 100%;
    display: flex;
    flex-direction: column;
    font-size: var(--font-size-sm);
  }
  
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    font-weight: 600;
    font-size: 11px;
    letter-spacing: 0.5px;
    color: var(--text-secondary);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-actions {
    display: flex;
    gap: 4px;
  }

  .header button {
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 2px 6px;
    font-size: 12px;
  }

  .header button:hover {
    background: var(--bg-hover);
    border-radius: 3px;
  }

  .git-branch {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    background: var(--bg-tertiary);
    border-radius: 3px;
    font-size: 10px;
    font-weight: 500;
    color: #4ec9b0;
  }

  .branch-icon {
    font-size: 10px;
  }
  
  .tree {
    flex: 1;
    overflow: auto;
  }
  
  .entry {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    cursor: pointer;
  }
  
  .entry:hover {
    background: var(--bg-hover);
  }

  .entry.selected {
    background: var(--bg-active, rgba(255, 255, 255, 0.1));
  }
  
  .icon {
    font-size: 14px;
  }
  
  .name {
    flex: 1;
  }

  .git-indicator {
    font-size: 10px;
    font-weight: 700;
    font-family: monospace;
    margin-left: auto;
    padding: 0 4px;
  }

  .entry.git-modified .name {
    color: #e5c07b;
  }

  .entry.git-staged .name {
    color: #4ec9b0;
  }

  .entry.git-untracked .name {
    color: var(--text-muted);
    font-style: italic;
  }
  
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  
  .empty button {
    background: var(--accent);
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 4px;
    cursor: pointer;
  }
</style>
