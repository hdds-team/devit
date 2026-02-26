<script lang="ts">
  import Modal from './Modal.svelte';
  import { llm, type Provider } from '../lib/tauri-ipc';
  import { selectedProviderStore, selectedModelStore } from '../lib/stores';

  interface Props {
    open: boolean;
    onClose: () => void;
    onSelect: (providerId: string) => void;
  }

  let { open, onClose, onSelect }: Props = $props();

  let providers = $state<Provider[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // Reload providers each time modal opens
  $effect(() => {
    if (open) {
      loadProviders();
    }
  });

  async function loadProviders() {
    loading = true;
    error = null;
    try {
      providers = await llm.listProviders();
    } catch (e) {
      console.error('Failed to load providers:', e);
      error = e instanceof Error ? e.message : 'Failed to load providers';
      // Fallback
      providers = [
        { id: 'ollama', name: 'Ollama', kind: 'local', available: false },
        { id: 'lmstudio', name: 'LM Studio', kind: 'local', available: false },
      ];
    }
    loading = false;
  }

  async function selectProvider(provider: Provider) {
    if (!provider.available) return;
    try {
      // Update backend
      await llm.setProvider(provider.id);
      // Update shared store (Chat.svelte subscribes to this)
      selectedProviderStore.set(provider.id);

      // Also load and set the first model for this provider
      try {
        const models = await llm.listModels(provider.id);
        if (models.length > 0) {
          await llm.setModel(models[0].name);
          selectedModelStore.set(models[0].name);
        }
      } catch (e) {
        console.error('Failed to load models for provider:', e);
      }

      onSelect(provider.id);
      onClose();
    } catch (e) {
      console.error('Failed to set provider:', e);
    }
  }
</script>

<Modal title="Select LLM Provider" {open} {onClose}>
  {#if loading}
    <p class="loading">Detecting providers...</p>
  {:else}
    {#if error}
      <div class="error-banner">{error}</div>
    {/if}
    <div class="providers">
      {#each providers as provider}
        <button
          class="provider"
          class:available={provider.available}
          class:unavailable={!provider.available}
          onclick={() => selectProvider(provider)}
          disabled={!provider.available}
        >
          <div class="provider-icon">
            {#if provider.id === 'ollama'}
              <span class="icon">🦙</span>
            {:else if provider.id === 'lmstudio'}
              <span class="icon">🎛️</span>
            {:else}
              <span class="icon">🤖</span>
            {/if}
          </div>
          <div class="provider-info">
            <span class="name">{provider.name}</span>
            <span class="kind">{provider.kind === 'local' ? 'Local' : 'Cloud'}</span>
          </div>
          <div class="status">
            {#if provider.available}
              <span class="dot available"></span>
              <span>Available</span>
            {:else}
              <span class="dot unavailable"></span>
              <span>Not running</span>
            {/if}
          </div>
        </button>
      {/each}
    </div>

    <div class="footer">
      <div class="hint">
        <p>Local providers run on your machine. Make sure the service is running before selecting.</p>
      </div>
      <button class="refresh-btn" onclick={loadProviders} disabled={loading}>
        {loading ? '↻ Refreshing...' : '↻ Refresh'}
      </button>
    </div>
  {/if}
</Modal>

<style>
  .loading {
    text-align: center;
    color: var(--text-muted);
    padding: 20px;
  }

  .error-banner {
    padding: 10px 12px;
    margin-bottom: 12px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid #ef4444;
    border-radius: 6px;
    color: #ef4444;
    font-size: 13px;
  }

  .providers {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .provider {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    transition: all 0.15s;
  }

  .provider.available:hover {
    border-color: var(--accent);
    background: var(--bg-hover);
  }

  .provider.unavailable {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .provider-icon {
    font-size: 28px;
  }

  .provider-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .name {
    font-weight: 600;
    color: var(--text-primary);
  }

  .kind {
    font-size: 12px;
    color: var(--text-muted);
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .dot.available {
    background: #22c55e;
  }

  .dot.unavailable {
    background: #ef4444;
  }

  .footer {
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .hint {
    flex: 1;
  }

  .hint p {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .refresh-btn {
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 6px 12px;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    white-space: nowrap;
  }

  .refresh-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
