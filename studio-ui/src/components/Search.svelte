<script lang="ts">
  import { workspace, type SearchResult } from '../lib/tauri-ipc';

  interface Props {
    onFileSelect?: (path: string, line: number, column: number) => void;
  }

  let { onFileSelect }: Props = $props();

  let searchQuery = $state('');
  let globPattern = $state('');
  let caseSensitive = $state(false);
  let isSearching = $state(false);
  let results = $state<SearchResult[]>([]);
  let error = $state<string | null>(null);
  let searchTime = $state<number | null>(null);

  // Group results by file
  let groupedResults = $derived(() => {
    const groups = new Map<string, SearchResult[]>();
    for (const result of results) {
      const existing = groups.get(result.path) || [];
      existing.push(result);
      groups.set(result.path, existing);
    }
    return Array.from(groups.entries());
  });

  async function handleSearch() {
    if (!searchQuery.trim()) {
      error = 'Enter a search pattern';
      return;
    }

    isSearching = true;
    error = null;
    results = [];
    searchTime = null;

    const startTime = performance.now();

    try {
      results = await workspace.searchInFiles(
        searchQuery,
        globPattern || undefined,
        caseSensitive
      );
      searchTime = Math.round(performance.now() - startTime);
    } catch (e: any) {
      error = e.toString();
    } finally {
      isSearching = false;
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      handleSearch();
    }
  }

  function handleResultClick(result: SearchResult) {
    onFileSelect?.(result.path, result.line, result.column);
  }

  function getFileName(path: string): string {
    return path.split('/').pop() || path;
  }

  function getRelativePath(path: string): string {
    // Show last 3 parts of path
    const parts = path.split('/');
    if (parts.length > 3) {
      return '.../' + parts.slice(-3).join('/');
    }
    return path;
  }

  function highlightMatch(text: string, match: string): string {
    if (!match) return escapeHtml(text);
    const escaped = escapeHtml(text);
    const escapedMatch = escapeHtml(match);
    return escaped.replace(
      new RegExp(escapeRegex(escapedMatch), 'gi'),
      '<mark>$&</mark>'
    );
  }

  function escapeHtml(str: string): string {
    return str
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');
  }

  function escapeRegex(str: string): string {
    return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }
</script>

<div class="search-panel">
  <div class="search-header">
    <div class="search-input-row">
      <input
        type="text"
        class="search-input"
        placeholder="Search pattern (regex)"
        bind:value={searchQuery}
        onkeydown={handleKeyDown}
      />
      <button
        class="search-btn"
        onclick={handleSearch}
        disabled={isSearching}
      >
        {isSearching ? 'Searching...' : 'Search'}
      </button>
    </div>
    <div class="search-options">
      <input
        type="text"
        class="glob-input"
        placeholder="File pattern (e.g. *.rs, *.ts)"
        bind:value={globPattern}
        onkeydown={handleKeyDown}
      />
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={caseSensitive} />
        Case sensitive
      </label>
    </div>
  </div>

  <div class="search-results">
    {#if error}
      <div class="error">{error}</div>
    {:else if searchTime !== null}
      <div class="search-stats">
        {results.length} results in {groupedResults().length} files ({searchTime}ms)
      </div>
    {/if}

    {#if results.length > 0}
      <div class="results-list">
        {#each groupedResults() as [path, fileResults]}
          <div class="file-group">
            <div class="file-header" title={path}>
              <span class="file-icon">&#128196;</span>
              <span class="file-name">{getFileName(path)}</span>
              <span class="file-path">{getRelativePath(path)}</span>
              <span class="match-count">{fileResults.length}</span>
            </div>
            <div class="file-matches">
              {#each fileResults as result}
                <button
                  class="match-row"
                  onclick={() => handleResultClick(result)}
                >
                  <span class="line-number">{result.line}</span>
                  <span class="match-text">{@html highlightMatch(result.text, result.match_text)}</span>
                </button>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    {:else if searchTime !== null && !isSearching}
      <div class="no-results">No results found</div>
    {:else if !isSearching}
      <div class="placeholder">
        <p>Search across all files in your workspace</p>
        <p class="hint">Tip: Use regex patterns like <code>fn\s+\w+</code></p>
      </div>
    {/if}
  </div>
</div>

<style>
  .search-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-secondary);
  }

  .search-header {
    padding: 8px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .search-input-row {
    display: flex;
    gap: 6px;
  }

  .search-input {
    flex: 1;
    padding: 6px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
  }

  .search-input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .glob-input {
    flex: 1;
    padding: 4px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .glob-input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .search-btn {
    padding: 6px 14px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 500;
  }

  .search-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .search-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .search-options {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    white-space: nowrap;
  }

  .checkbox-label input {
    margin: 0;
  }

  .search-results {
    flex: 1;
    overflow-y: auto;
    padding: 4px;
  }

  .search-stats {
    padding: 6px 8px;
    font-size: 11px;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border);
  }

  .error {
    padding: 12px;
    color: #dc3545;
    font-size: 12px;
  }

  .results-list {
    display: flex;
    flex-direction: column;
  }

  .file-group {
    border-bottom: 1px solid var(--border);
  }

  .file-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    background: var(--bg-tertiary, rgba(0, 0, 0, 0.1));
    font-size: 12px;
    cursor: default;
  }

  .file-icon {
    font-size: 14px;
    opacity: 0.7;
  }

  .file-name {
    color: var(--text-primary);
    font-weight: 500;
  }

  .file-path {
    color: var(--text-muted);
    font-size: 10px;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .match-count {
    background: var(--accent);
    color: white;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 10px;
  }

  .file-matches {
    display: flex;
    flex-direction: column;
  }

  .match-row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 4px 8px 4px 24px;
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    color: var(--text-secondary);
    width: 100%;
  }

  .match-row:hover {
    background: var(--bg-hover);
  }

  .line-number {
    color: var(--text-muted);
    min-width: 40px;
    text-align: right;
    font-size: 11px;
  }

  .match-text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .match-text :global(mark) {
    background: rgba(255, 193, 7, 0.3);
    color: var(--text-primary);
    padding: 0 2px;
    border-radius: 2px;
  }

  .no-results {
    padding: 20px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  .placeholder {
    padding: 20px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }

  .placeholder p {
    margin: 0 0 8px 0;
  }

  .hint {
    font-size: 11px;
  }

  .hint code {
    background: var(--bg-primary);
    padding: 2px 6px;
    border-radius: 3px;
    font-family: var(--font-mono, monospace);
  }
</style>
