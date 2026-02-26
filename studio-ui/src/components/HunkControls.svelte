<script lang="ts">
  import { structuredEdit, type EditHunk, type StructuredEditResponse } from '../lib/tauri-ipc';

  // Props
  let { session = $bindable<StructuredEditResponse | null>(null), onClose = () => {} } = $props<{
    session?: StructuredEditResponse | null;
    onClose?: () => void;
  }>();

  // State
  let selectedHunkIndex = $state(0);
  let isApplying = $state(false);
  let error = $state<string | null>(null);

  // Derived
  let hunks = $derived(session?.hunks ?? []);
  let currentHunk = $derived(hunks[selectedHunkIndex]);
  let acceptedCount = $derived(hunks.filter((h: EditHunk) => h.status === 'accepted').length);
  let rejectedCount = $derived(hunks.filter((h: EditHunk) => h.status === 'rejected').length);
  let pendingCount = $derived(hunks.filter((h: EditHunk) => h.status === 'pending').length);

  // Navigate hunks
  function nextHunk() {
    if (selectedHunkIndex < hunks.length - 1) {
      selectedHunkIndex++;
    }
  }

  function prevHunk() {
    if (selectedHunkIndex > 0) {
      selectedHunkIndex--;
    }
  }

  // Accept/reject current hunk
  async function acceptHunk() {
    if (!session || !currentHunk) return;
    try {
      await structuredEdit.updateHunkStatus(session.sessionId, currentHunk.id, true);
      // Update local state
      session.hunks = session.hunks.map((h: EditHunk) =>
        h.id === currentHunk.id ? { ...h, status: 'accepted' as const } : h
      );
      // Auto-advance to next pending
      advanceToNextPending();
    } catch (e) {
      error = String(e);
    }
  }

  async function rejectHunk() {
    if (!session || !currentHunk) return;
    try {
      await structuredEdit.updateHunkStatus(session.sessionId, currentHunk.id, false);
      // Update local state
      session.hunks = session.hunks.map((h: EditHunk) =>
        h.id === currentHunk.id ? { ...h, status: 'rejected' as const } : h
      );
      // Auto-advance to next pending
      advanceToNextPending();
    } catch (e) {
      error = String(e);
    }
  }

  function advanceToNextPending() {
    // Find next pending hunk
    for (let i = selectedHunkIndex + 1; i < hunks.length; i++) {
      if (hunks[i].status === 'pending') {
        selectedHunkIndex = i;
        return;
      }
    }
    // Wrap around
    for (let i = 0; i < selectedHunkIndex; i++) {
      if (hunks[i].status === 'pending') {
        selectedHunkIndex = i;
        return;
      }
    }
  }

  // Accept all remaining
  async function acceptAll() {
    if (!session) return;
    for (const hunk of hunks) {
      if (hunk.status === 'pending') {
        await structuredEdit.updateHunkStatus(session.sessionId, hunk.id, true);
      }
    }
    session.hunks = session.hunks.map((h: EditHunk) =>
      h.status === 'pending' ? { ...h, status: 'accepted' as const } : h
    );
  }

  // Reject all remaining
  async function rejectAll() {
    if (!session) return;
    for (const hunk of hunks) {
      if (hunk.status === 'pending') {
        await structuredEdit.updateHunkStatus(session.sessionId, hunk.id, false);
      }
    }
    session.hunks = session.hunks.map((h: EditHunk) =>
      h.status === 'pending' ? { ...h, status: 'rejected' as const } : h
    );
  }

  // Apply accepted hunks
  async function applyChanges() {
    if (!session || acceptedCount === 0) return;
    isApplying = true;
    error = null;
    try {
      const modifiedFiles = await structuredEdit.applyAccepted(session.sessionId);
      // Update status to applied
      session.hunks = session.hunks.map((h: EditHunk) =>
        h.status === 'accepted' ? { ...h, status: 'applied' as const } : h
      );
      // Close if all done
      if (modifiedFiles.length > 0) {
        onClose();
      }
    } catch (e) {
      error = String(e);
    } finally {
      isApplying = false;
    }
  }

  // Cancel session
  async function cancel() {
    if (session) {
      await structuredEdit.cancel(session.sessionId);
    }
    onClose();
  }

  // Keyboard shortcuts
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault();
      acceptHunk();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      rejectHunk();
    } else if (e.key === 'ArrowDown' || e.key === 'j') {
      e.preventDefault();
      nextHunk();
    } else if (e.key === 'ArrowUp' || e.key === 'k') {
      e.preventDefault();
      prevHunk();
    } else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      applyChanges();
    }
  }

  // Format diff for display
  function formatDiff(original: string, replacement: string): { type: 'context' | 'delete' | 'insert'; content: string }[] {
    const lines: { type: 'context' | 'delete' | 'insert'; content: string }[] = [];

    const origLines = original.split('\n');
    const newLines = replacement.split('\n');

    // Simple diff: show all original as deletions, all new as insertions
    // For a real implementation, use a proper diff algorithm
    for (const line of origLines) {
      if (line.trim()) {
        lines.push({ type: 'delete', content: line });
      }
    }
    for (const line of newLines) {
      if (line.trim()) {
        lines.push({ type: 'insert', content: line });
      }
    }

    return lines;
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if session && hunks.length > 0}
  <div class="hunk-controls">
    <!-- Header -->
    <div class="header">
      <div class="title">
        <span class="icon">&#x1F527;</span>
        Code Changes ({hunks.length} hunks)
      </div>
      <div class="stats">
        <span class="stat accepted">{acceptedCount} accepted</span>
        <span class="stat rejected">{rejectedCount} rejected</span>
        <span class="stat pending">{pendingCount} pending</span>
      </div>
      <button class="close-btn" onclick={cancel}>&times;</button>
    </div>

    <!-- Current hunk display -->
    {#if currentHunk}
      <div class="hunk-info">
        <span class="file-path">{currentHunk.filePath}</span>
        <span class="line-range">Lines {currentHunk.startLine}-{currentHunk.endLine}</span>
        <span class="hunk-nav">
          {selectedHunkIndex + 1} / {hunks.length}
        </span>
      </div>

      <div class="description">{currentHunk.description}</div>

      <!-- Diff view -->
      <div class="diff-view">
        {#each formatDiff(currentHunk.original, currentHunk.replacement) as line}
          <div class="diff-line {line.type}">
            <span class="prefix">{line.type === 'delete' ? '-' : line.type === 'insert' ? '+' : ' '}</span>
            <span class="content">{line.content}</span>
          </div>
        {/each}
      </div>

      <!-- Hunk status badge -->
      <div class="hunk-status {currentHunk.status}">
        {currentHunk.status.toUpperCase()}
      </div>
    {/if}

    <!-- Controls -->
    <div class="controls">
      <div class="nav-controls">
        <button onclick={prevHunk} disabled={selectedHunkIndex === 0}>
          <span class="key">&#x2191;</span> Prev
        </button>
        <button onclick={nextHunk} disabled={selectedHunkIndex >= hunks.length - 1}>
          <span class="key">&#x2193;</span> Next
        </button>
      </div>

      <div class="action-controls">
        <button class="reject" onclick={rejectHunk} disabled={currentHunk?.status !== 'pending'}>
          <span class="key">Esc</span> Reject
        </button>
        <button class="accept" onclick={acceptHunk} disabled={currentHunk?.status !== 'pending'}>
          <span class="key">Tab</span> Accept
        </button>
      </div>

      <div class="bulk-controls">
        <button onclick={acceptAll} disabled={pendingCount === 0}>Accept All</button>
        <button onclick={rejectAll} disabled={pendingCount === 0}>Reject All</button>
      </div>
    </div>

    <!-- Apply button -->
    <div class="apply-section">
      <button
        class="apply-btn"
        onclick={applyChanges}
        disabled={acceptedCount === 0 || isApplying}
      >
        {#if isApplying}
          Applying...
        {:else}
          <span class="key">Cmd+Enter</span> Apply {acceptedCount} Changes
        {/if}
      </button>
    </div>

    <!-- Error display -->
    {#if error}
      <div class="error">{error}</div>
    {/if}
  </div>
{/if}

<style>
  .hunk-controls {
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary, #1e1e1e);
    border: 1px solid var(--border-color, #333);
    border-radius: 6px;
    padding: 12px;
    font-family: var(--font-mono, 'JetBrains Mono', monospace);
    font-size: 13px;
    max-height: 500px;
    overflow: hidden;
  }

  .header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-color, #333);
  }

  .title {
    font-weight: 600;
    flex: 1;
  }

  .icon {
    margin-right: 6px;
  }

  .stats {
    display: flex;
    gap: 8px;
    font-size: 11px;
  }

  .stat {
    padding: 2px 6px;
    border-radius: 3px;
  }

  .stat.accepted {
    background: rgba(0, 200, 83, 0.2);
    color: #00c853;
  }

  .stat.rejected {
    background: rgba(255, 82, 82, 0.2);
    color: #ff5252;
  }

  .stat.pending {
    background: rgba(255, 193, 7, 0.2);
    color: #ffc107;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-secondary, #888);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
  }

  .close-btn:hover {
    color: var(--text-primary, #fff);
  }

  .hunk-info {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    font-size: 12px;
  }

  .file-path {
    color: var(--accent, #569cd6);
    flex: 1;
  }

  .line-range {
    color: var(--text-secondary, #888);
  }

  .hunk-nav {
    color: var(--text-secondary, #888);
    background: var(--bg-tertiary, #2d2d2d);
    padding: 2px 8px;
    border-radius: 3px;
  }

  .description {
    color: var(--text-secondary, #aaa);
    font-size: 12px;
    padding: 4px 0 8px;
    font-style: italic;
  }

  .diff-view {
    flex: 1;
    overflow: auto;
    background: var(--bg-tertiary, #1a1a1a);
    border-radius: 4px;
    padding: 8px;
    max-height: 200px;
  }

  .diff-line {
    display: flex;
    font-size: 12px;
    line-height: 1.5;
  }

  .diff-line.delete {
    background: rgba(255, 82, 82, 0.15);
    color: #ff8a80;
  }

  .diff-line.insert {
    background: rgba(0, 200, 83, 0.15);
    color: #69f0ae;
  }

  .diff-line.context {
    color: var(--text-secondary, #888);
  }

  .prefix {
    width: 20px;
    text-align: center;
    user-select: none;
    color: inherit;
    opacity: 0.7;
  }

  .content {
    flex: 1;
    white-space: pre;
  }

  .hunk-status {
    text-align: center;
    padding: 4px;
    font-size: 11px;
    font-weight: 600;
    border-radius: 3px;
    margin: 8px 0;
  }

  .hunk-status.pending {
    background: rgba(255, 193, 7, 0.2);
    color: #ffc107;
  }

  .hunk-status.accepted {
    background: rgba(0, 200, 83, 0.2);
    color: #00c853;
  }

  .hunk-status.rejected {
    background: rgba(255, 82, 82, 0.2);
    color: #ff5252;
  }

  .hunk-status.applied {
    background: rgba(33, 150, 243, 0.2);
    color: #2196f3;
  }

  .controls {
    display: flex;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--border-color, #333);
  }

  .nav-controls,
  .action-controls,
  .bulk-controls {
    display: flex;
    gap: 6px;
  }

  .action-controls {
    flex: 1;
    justify-content: center;
  }

  button {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 12px;
    border: 1px solid var(--border-color, #444);
    border-radius: 4px;
    background: var(--bg-tertiary, #2d2d2d);
    color: var(--text-primary, #fff);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  button:hover:not(:disabled) {
    background: var(--bg-hover, #3d3d3d);
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  button.accept {
    background: rgba(0, 200, 83, 0.2);
    border-color: #00c853;
    color: #00c853;
  }

  button.accept:hover:not(:disabled) {
    background: rgba(0, 200, 83, 0.3);
  }

  button.reject {
    background: rgba(255, 82, 82, 0.2);
    border-color: #ff5252;
    color: #ff5252;
  }

  button.reject:hover:not(:disabled) {
    background: rgba(255, 82, 82, 0.3);
  }

  .key {
    font-size: 10px;
    padding: 1px 4px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 2px;
    font-family: var(--font-mono);
  }

  .apply-section {
    padding-top: 8px;
    border-top: 1px solid var(--border-color, #333);
  }

  .apply-btn {
    width: 100%;
    padding: 10px;
    background: var(--accent, #569cd6);
    border: none;
    color: #fff;
    font-weight: 600;
  }

  .apply-btn:hover:not(:disabled) {
    background: var(--accent-hover, #6cb2eb);
  }

  .error {
    margin-top: 8px;
    padding: 8px;
    background: rgba(255, 82, 82, 0.2);
    color: #ff5252;
    border-radius: 4px;
    font-size: 12px;
  }
</style>
