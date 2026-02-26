<script lang="ts">
  interface Props {
    onAction?: (action: string) => void;
  }

  let { onAction }: Props = $props();

  let openMenu = $state<string | null>(null);

  function toggleMenu(menu: string) {
    openMenu = openMenu === menu ? null : menu;
  }

  function closeMenu() {
    openMenu = null;
  }

  function handleAction(action: string) {
    console.log('Menu action:', action);
    closeMenu();

    // Emit to parent for handled actions
    if (onAction) {
      onAction(action);
      return;
    }

    // Fallback for unhandled actions
    switch (action) {
      case 'about':
        alert('devit-studio v0.1.0\nA local-first IDE with integrated LLM');
        break;
      default:
        console.log('Unhandled action:', action);
    }
  }

  const menus: Record<string, { label: string; action: string }[]> = {
    file: [
      { label: 'New File', action: 'new-file' },
      { label: 'Open File...', action: 'open-file' },
      { label: 'Open Folder...', action: 'open-folder' },
      { label: 'Save', action: 'save' },
      { label: 'Save As...', action: 'save-as' },
      { label: 'Reload from Disk', action: 'reload' },
    ],
    edit: [
      { label: 'Undo', action: 'undo' },
      { label: 'Redo', action: 'redo' },
      { label: 'Cut', action: 'cut' },
      { label: 'Copy', action: 'copy' },
      { label: 'Paste', action: 'paste' },
      { label: 'Find', action: 'find' },
    ],
    view: [
      { label: 'Toggle Explorer', action: 'toggle-explorer' },
      { label: 'Toggle Chat', action: 'toggle-chat' },
      { label: 'Toggle Terminal', action: 'toggle-terminal' },
      { label: 'Zoom In', action: 'zoom-in' },
      { label: 'Zoom Out', action: 'zoom-out' },
    ],
    llm: [
      { label: 'Select Provider...', action: 'select-provider' },
      { label: 'Model Settings', action: 'model-settings' },
      { label: 'Clear Chat', action: 'clear-chat' },
    ],
    help: [
      { label: 'Documentation', action: 'docs' },
      { label: 'Keyboard Shortcuts', action: 'shortcuts' },
      { label: 'About', action: 'about' },
    ],
  };
</script>

<svelte:window onclick={closeMenu} />

<header class="toolbar">
  <div class="menu">
    {#each Object.entries(menus) as [key, items]}
      <div class="menu-item">
        <button
          onclick={(e) => { e.stopPropagation(); toggleMenu(key); }}
          class:active={openMenu === key}
        >
          {key.charAt(0).toUpperCase() + key.slice(1)}
        </button>
        {#if openMenu === key}
          <div class="dropdown" onclick={(e) => e.stopPropagation()}>
            {#each items as item}
              <button class="dropdown-item" onclick={() => handleAction(item.action)}>
                {item.label}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
  <div class="spacer"></div>
  <div class="title">devit-studio</div>
  <div class="spacer"></div>
  <div class="actions">
    <button class="icon" title="Settings" onclick={() => handleAction('settings')}>⚙</button>
  </div>
</header>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    height: 32px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border);
    padding: 0 8px;
    font-size: var(--font-size-sm);
    -webkit-app-region: drag;
  }

  .menu {
    display: flex;
    gap: 0;
    -webkit-app-region: no-drag;
  }

  .menu-item {
    position: relative;
  }

  .menu-item > button {
    background: transparent;
    border: none;
    color: var(--text-primary);
    padding: 6px 12px;
    cursor: pointer;
  }

  .menu-item > button:hover,
  .menu-item > button.active {
    background: var(--bg-hover);
  }

  .dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    min-width: 180px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 4px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 1000;
    padding: 4px 0;
  }

  .dropdown-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text-primary);
    padding: 8px 16px;
    cursor: pointer;
    font-size: var(--font-size-sm);
  }

  .dropdown-item:hover {
    background: var(--bg-hover);
  }

  .title {
    font-weight: 500;
    color: var(--text-secondary);
  }

  .spacer {
    flex: 1;
  }

  .actions {
    -webkit-app-region: no-drag;
  }

  .icon {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    padding: 4px 8px;
    cursor: pointer;
  }

  .icon:hover {
    color: var(--text-primary);
  }
</style>
