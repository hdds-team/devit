<script lang="ts">
  interface Props {
    onAction?: (action: string) => void;
    currentFile?: string | null;
    gitStatus?: { added: number; modified: number; deleted: number } | null;
  }

  let { onAction, currentFile = null, gitStatus = null }: Props = $props();

  function handleAction(action: string) {
    onAction?.(action);
  }

  // Extract breadcrumb parts from file path
  function getBreadcrumbs(path: string | null): string[] {
    if (!path) return [];
    // Get last 3 parts of path for compact display
    const parts = path.split('/').filter(Boolean);
    return parts.slice(-3);
  }

  let breadcrumbs = $derived(getBreadcrumbs(currentFile));
</script>

<div class="action-bar">
  <!-- File actions -->
  <div class="action-group">
    <button class="action-btn" title="New File (Ctrl+N)" onclick={() => handleAction('new-file')}>
      <span class="icon">+</span>
    </button>
    <button class="action-btn" title="Open File (Ctrl+O)" onclick={() => handleAction('open-file')}>
      <span class="icon">&#128194;</span>
    </button>
    <button class="action-btn" title="Save (Ctrl+S)" onclick={() => handleAction('save')}>
      <span class="icon">&#128190;</span>
    </button>
  </div>

  <div class="separator"></div>

  <!-- Edit actions -->
  <div class="action-group">
    <button class="action-btn" title="Undo (Ctrl+Z)" onclick={() => handleAction('undo')}>
      <span class="icon">&#8630;</span>
    </button>
    <button class="action-btn" title="Redo (Ctrl+Y)" onclick={() => handleAction('redo')}>
      <span class="icon">&#8631;</span>
    </button>
  </div>

  <div class="separator"></div>

  <!-- Search actions -->
  <div class="action-group">
    <button class="action-btn" title="Find (Ctrl+F)" onclick={() => handleAction('find')}>
      <span class="icon">&#128269;</span>
    </button>
    <button class="action-btn" title="Find in Files (Ctrl+Shift+F)" onclick={() => handleAction('find-in-files')}>
      <span class="icon">&#128270;</span>
    </button>
  </div>

  <div class="separator"></div>

  <!-- View toggles -->
  <div class="action-group">
    <button class="action-btn" title="Toggle Explorer" onclick={() => handleAction('toggle-explorer')}>
      <span class="icon">&#128193;</span>
    </button>
    <button class="action-btn" title="Toggle Terminal" onclick={() => handleAction('toggle-terminal')}>
      <span class="icon">&#9638;</span>
    </button>
    <button class="action-btn" title="Toggle Chat" onclick={() => handleAction('toggle-chat')}>
      <span class="icon">&#128172;</span>
    </button>
    <button class="action-btn" title="Split Editor (coming soon)" onclick={() => handleAction('split-editor')} disabled>
      <span class="icon">&#9707;</span>
    </button>
  </div>

  <div class="separator"></div>

  <!-- Git status (if available) -->
  {#if gitStatus}
    <div class="action-group git-status">
      <button class="action-btn git-btn" title="Git Status" onclick={() => handleAction('git-status')}>
        <span class="icon">&#9733;</span>
        {#if gitStatus.added > 0}
          <span class="git-badge added">+{gitStatus.added}</span>
        {/if}
        {#if gitStatus.modified > 0}
          <span class="git-badge modified">~{gitStatus.modified}</span>
        {/if}
        {#if gitStatus.deleted > 0}
          <span class="git-badge deleted">-{gitStatus.deleted}</span>
        {/if}
      </button>
    </div>
    <div class="separator"></div>
  {/if}

  <!-- Breadcrumbs -->
  <div class="breadcrumbs">
    {#if breadcrumbs.length > 0}
      {#each breadcrumbs as part, i}
        {#if i > 0}
          <span class="breadcrumb-sep">/</span>
        {/if}
        <button
          class="breadcrumb"
          class:current={i === breadcrumbs.length - 1}
          onclick={() => handleAction(`breadcrumb:${i}`)}
        >
          {part}
        </button>
      {/each}
    {:else}
      <span class="breadcrumb-empty">No file open</span>
    {/if}
  </div>

  <div class="spacer"></div>

  <!-- Right side actions -->
  <div class="action-group">
    <button class="action-btn" title="Reload from Disk (F5)" onclick={() => handleAction('reload')}>
      <span class="icon">&#8635;</span>
    </button>
  </div>
</div>

<style>
  .action-bar {
    display: flex;
    align-items: center;
    height: 28px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    padding: 0 8px;
    gap: 4px;
    font-size: 12px;
  }

  .action-group {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .action-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .action-btn:active:not(:disabled) {
    background: var(--bg-active);
  }

  .action-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .action-btn .icon {
    font-size: 14px;
    line-height: 1;
  }

  .separator {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 4px;
  }

  .breadcrumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 8px;
    overflow: hidden;
  }

  .breadcrumb {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    padding: 2px 6px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 11px;
    white-space: nowrap;
  }

  .breadcrumb:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .breadcrumb.current {
    color: var(--text-primary);
    font-weight: 500;
  }

  .breadcrumb-sep {
    color: var(--text-muted);
    font-size: 10px;
  }

  .breadcrumb-empty {
    color: var(--text-muted);
    font-style: italic;
    font-size: 11px;
  }

  .spacer {
    flex: 1;
  }

  /* Git status badges */
  .git-btn {
    width: auto;
    padding: 0 6px;
    gap: 4px;
  }

  .git-badge {
    font-size: 10px;
    padding: 1px 4px;
    border-radius: 3px;
    font-weight: 500;
  }

  .git-badge.added {
    background: rgba(40, 167, 69, 0.2);
    color: #28a745;
  }

  .git-badge.modified {
    background: rgba(255, 193, 7, 0.2);
    color: #ffc107;
  }

  .git-badge.deleted {
    background: rgba(220, 53, 69, 0.2);
    color: #dc3545;
  }
</style>
