<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import Modal from './Modal.svelte';
  import { llm, type ModelInfo } from '../lib/tauri-ipc';
  import { selectedModelStore } from '../lib/stores';

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  // Available models from backend
  let models = $state<ModelInfo[]>([]);
  let loadingModels = $state(false);
  let modelsError = $state<string | null>(null);

  // Model settings state
  let modelName = $state('');
  let temperature = $state(0.7);
  let maxTokens = $state(2048);
  let topP = $state(0.9);
  let systemPrompt = $state('You are a helpful coding assistant.');

  // Load models and sync with store when modal opens
  $effect(() => {
    if (open) {
      // Get current value from store (one-time read)
      const storeValue = get(selectedModelStore);
      if (storeValue) {
        modelName = storeValue;
      }

      // Always reload models when modal opens (provider might have changed)
      loadModels();
    }
  });

  async function loadModels() {
    loadingModels = true;
    modelsError = null;
    try {
      models = await llm.listModels();
      if (models.length > 0 && !modelName) {
        modelName = models[0].name;
      }
    } catch (e) {
      modelsError = e instanceof Error ? e.message : 'Failed to load models';
      console.error('Failed to load models:', e);
    }
    loadingModels = false;
  }

  async function handleSave() {
    // Save model selection to backend and store
    if (modelName) {
      await llm.setModel(modelName);
      selectedModelStore.set(modelName);
    }
    console.log('Model settings saved:', {
      modelName,
      temperature,
      maxTokens,
      topP,
      systemPrompt,
    });
    onClose();
  }

  function handleReset() {
    modelName = models.length > 0 ? models[0].name : '';
    temperature = 0.7;
    maxTokens = 2048;
    topP = 0.9;
    systemPrompt = 'You are a helpful coding assistant.';
  }
</script>

<Modal title="Model Settings" {open} {onClose}>
  <div class="form">
    <div class="field">
      <label for="model">Model</label>
      {#if loadingModels}
        <div class="loading-models">Loading models...</div>
      {:else if modelsError}
        <div class="error">{modelsError}</div>
        <button class="retry" onclick={loadModels}>Retry</button>
      {:else if models.length > 0}
        <select id="model" bind:value={modelName}>
          {#each models as model}
            <option value={model.name}>{model.name} ({model.size})</option>
          {/each}
        </select>
      {:else}
        <div class="no-models">No models found. Run `ollama pull codellama` to download a model.</div>
      {/if}
    </div>

    <div class="field">
      <label for="temperature">Temperature: {temperature.toFixed(2)}</label>
      <input
        id="temperature"
        type="range"
        min="0"
        max="2"
        step="0.1"
        bind:value={temperature}
      />
      <div class="range-labels">
        <span>Precise</span>
        <span>Creative</span>
      </div>
    </div>

    <div class="field">
      <label for="max-tokens">Max Tokens</label>
      <input id="max-tokens" type="number" bind:value={maxTokens} min="256" max="8192" step="256" />
      <span class="hint">Maximum length of generated response</span>
    </div>

    <div class="field">
      <label for="top-p">Top P: {topP.toFixed(2)}</label>
      <input
        id="top-p"
        type="range"
        min="0"
        max="1"
        step="0.05"
        bind:value={topP}
      />
    </div>

    <div class="field">
      <label for="system">System Prompt</label>
      <textarea id="system" bind:value={systemPrompt} rows="3"></textarea>
    </div>

    <div class="actions">
      <button class="reset" onclick={handleReset}>Reset to Defaults</button>
      <div class="spacer"></div>
      <button class="secondary" onclick={onClose}>Cancel</button>
      <button class="primary" onclick={handleSave}>Save</button>
    </div>
  </div>
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  input[type="text"],
  input[type="number"],
  textarea {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 10px 12px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
  }

  input:focus,
  textarea:focus {
    border-color: var(--accent);
    outline: none;
  }

  textarea {
    resize: vertical;
    min-height: 60px;
  }

  input[type="range"] {
    width: 100%;
    height: 6px;
    border-radius: 3px;
    background: var(--bg-tertiary);
    cursor: pointer;
  }

  .range-labels {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--text-muted);
  }

  .hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  .loading-models, .no-models {
    padding: 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .error {
    padding: 12px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid #ef4444;
    border-radius: 4px;
    color: #ef4444;
    font-size: 13px;
  }

  .retry {
    margin-top: 8px;
    padding: 6px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 12px;
  }

  .retry:hover {
    background: var(--bg-hover);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }

  .spacer {
    flex: 1;
  }

  button {
    padding: 8px 16px;
    border-radius: 4px;
    font-size: 13px;
    cursor: pointer;
  }

  .reset {
    background: transparent;
    border: none;
    color: var(--text-muted);
    padding: 8px 0;
  }

  .reset:hover {
    color: var(--text-primary);
  }

  .secondary {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-primary);
  }

  .secondary:hover {
    background: var(--bg-hover);
  }

  .primary {
    background: var(--accent);
    border: none;
    color: white;
  }

  .primary:hover {
    opacity: 0.9;
  }
</style>
