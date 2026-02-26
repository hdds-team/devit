<script lang="ts">
  interface Props {
    activeView: string;
    agentsUp?: number;
    onViewChange: (view: string) => void;
  }

  let { activeView, agentsUp = 0, onViewChange }: Props = $props();

  const views = [
    { id: 'explorer', icon: '\u{1F4C1}', label: 'Explorer' },
    { id: 'search',   icon: '\u{1F50D}', label: 'Search' },
    { id: 'team',     icon: '\u{1F465}', label: 'Team' },
    { id: 'settings', icon: '\u2699',    label: 'Settings' },
  ];
</script>

<nav class="activity-bar">
  {#each views as view}
    <button
      class="activity-btn"
      class:active={activeView === view.id}
      title={view.label}
      onclick={() => onViewChange(activeView === view.id ? '' : view.id)}
    >
      <span class="icon">{view.icon}</span>
      {#if view.id === 'team' && agentsUp > 0}
        <span class="badge">{agentsUp}</span>
      {/if}
    </button>
  {/each}
</nav>

<style>
  .activity-bar {
    display: flex;
    flex-direction: column;
    width: 40px;
    background: var(--bg-tertiary, #1e1e1e);
    border-right: 1px solid var(--border, #3c3c3c);
    align-items: center;
    padding-top: 4px;
    gap: 2px;
    flex-shrink: 0;
  }

  .activity-btn {
    position: relative;
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-left: 2px solid transparent;
    color: var(--text-secondary, #969696);
    cursor: pointer;
    font-size: 16px;
    border-radius: 4px;
  }

  .activity-btn:hover {
    color: var(--text-primary, #cccccc);
    background: var(--bg-hover, rgba(255, 255, 255, 0.05));
  }

  .activity-btn.active {
    color: var(--text-primary, #cccccc);
    border-left-color: var(--accent, #007acc);
  }

  .icon {
    font-size: 16px;
    line-height: 1;
  }

  .badge {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 14px;
    height: 14px;
    background: #4caf50;
    color: #fff;
    font-size: 9px;
    font-weight: 700;
    border-radius: 7px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 3px;
    line-height: 1;
  }
</style>
