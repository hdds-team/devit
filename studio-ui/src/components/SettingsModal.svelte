<script lang="ts">
  import { onMount } from 'svelte';
  import Modal from './Modal.svelte';
  import { settings, llm, type Settings, type Provider } from '../lib/tauri-ipc';

  interface Props {
    open: boolean;
    onClose: () => void;
    onPreview?: (settings: Settings) => void;
  }

  let { open, onClose, onPreview }: Props = $props();

  let currentSettings = $state<Settings | null>(null);
  let originalSettings = $state<Settings | null>(null);
  let providers = $state<Provider[]>([]);
  let selectedProvider = $state<string>('ollama');
  let saving = $state(false);
  let syncFonts = $state(false);

  onMount(async () => {
    await loadSettings();
  });

  async function loadSettings() {
    try {
      const loaded = await settings.get();
      currentSettings = { ...loaded };
      originalSettings = { ...loaded };
      providers = await llm.listProviders();
      selectedProvider = currentSettings?.default_provider || 'ollama';
      // Check if fonts are already synced
      syncFonts = currentSettings.chat_font_size === currentSettings.font_size &&
                  currentSettings.chat_font_family === currentSettings.font_family;
    } catch (e) {
      console.error('Failed to load settings:', e);
    }
  }

  // Emit preview on any change
  function emitPreview() {
    if (currentSettings && onPreview) {
      onPreview(currentSettings);
    }
  }

  async function handleSave() {
    if (!currentSettings) return;
    saving = true;
    try {
      currentSettings.default_provider = selectedProvider;
      await settings.set(currentSettings);
      await llm.setProvider(selectedProvider);
      onClose();
    } catch (e) {
      console.error('Failed to save settings:', e);
    }
    saving = false;
  }

  function handleCancel() {
    // Restore original settings on cancel
    if (originalSettings && onPreview) {
      onPreview(originalSettings);
    }
    onClose();
  }

  function handleFontSizeChange(delta: number) {
    if (currentSettings) {
      currentSettings.font_size = Math.max(10, Math.min(32, currentSettings.font_size + delta));
      if (syncFonts) {
        currentSettings.chat_font_size = currentSettings.font_size;
      }
      emitPreview();
    }
  }

  function handleChatFontSizeChange(delta: number) {
    if (currentSettings) {
      currentSettings.chat_font_size = Math.max(10, Math.min(32, currentSettings.chat_font_size + delta));
      emitPreview();
    }
  }

  function setChatPreset(size: number) {
    if (currentSettings) {
      currentSettings.chat_font_size = size;
      emitPreview();
    }
  }

  function handleSyncToggle() {
    syncFonts = !syncFonts;
    if (syncFonts && currentSettings) {
      currentSettings.chat_font_size = currentSettings.font_size;
      currentSettings.chat_font_family = currentSettings.font_family;
      emitPreview();
    }
  }

  function handleChatFontFamilyChange(e: Event) {
    if (currentSettings) {
      currentSettings.chat_font_family = (e.target as HTMLInputElement).value;
      emitPreview();
    }
  }

  function handleFontFamilyChange(e: Event) {
    if (currentSettings) {
      currentSettings.font_family = (e.target as HTMLInputElement).value;
      if (syncFonts) {
        currentSettings.chat_font_family = currentSettings.font_family;
      }
      emitPreview();
    }
  }

  let defaultSystemPrompt = $state<string>('');

  // Load default prompt on mount
  $effect(() => {
    if (open && !defaultSystemPrompt) {
      llm.getDefaultSystemPrompt().then(p => {
        defaultSystemPrompt = p;
        // Pre-fill with default if no custom prompt set
        if (currentSettings && !currentSettings.system_prompt) {
          currentSettings.system_prompt = p;
        }
      });
    }
  });

  function handleSystemPromptChange(e: Event) {
    if (currentSettings) {
      const value = (e.target as HTMLTextAreaElement).value;
      currentSettings.system_prompt = value || undefined;
    }
  }

  function resetSystemPrompt() {
    if (currentSettings && defaultSystemPrompt) {
      currentSettings.system_prompt = defaultSystemPrompt;
    }
  }
</script>

<Modal title="Settings" {open} onClose={handleCancel}>
  {#if currentSettings}
    <div class="settings-form">
      <section>
        <h3>Appearance</h3>
        <div class="field">
          <label>Theme</label>
          <select bind:value={currentSettings.theme} disabled>
            <option value="dark">Dark</option>
          </select>
          <span class="hint">More themes coming soon</span>
        </div>

        <div class="field">
          <label>Editor Font Size</label>
          <div class="stepper">
            <button onclick={() => handleFontSizeChange(-1)}>-</button>
            <span>{currentSettings.font_size}px</span>
            <button onclick={() => handleFontSizeChange(1)}>+</button>
          </div>
        </div>

        <div class="field">
          <label>Editor Font Family</label>
          <input type="text" value={currentSettings.font_family} oninput={handleFontFamilyChange} />
        </div>
      </section>

      <section>
        <h3>Chat Panel</h3>

        <div class="field">
          <label class="checkbox-label">
            <input type="checkbox" checked={syncFonts} onchange={handleSyncToggle} />
            Sync with editor font
          </label>
        </div>

        {#if !syncFonts}
          <div class="field">
            <label>Font Size</label>
            <div class="size-controls">
              <div class="presets">
                <button
                  class="preset"
                  class:active={currentSettings.chat_font_size === 12}
                  onclick={() => setChatPreset(12)}
                >S</button>
                <button
                  class="preset"
                  class:active={currentSettings.chat_font_size === 14}
                  onclick={() => setChatPreset(14)}
                >M</button>
                <button
                  class="preset"
                  class:active={currentSettings.chat_font_size === 18}
                  onclick={() => setChatPreset(18)}
                >L</button>
              </div>
              <div class="stepper">
                <button onclick={() => handleChatFontSizeChange(-1)}>-</button>
                <span>{currentSettings.chat_font_size}px</span>
                <button onclick={() => handleChatFontSizeChange(1)}>+</button>
              </div>
            </div>
          </div>

          <div class="field">
            <label>Font Family</label>
            <input type="text" value={currentSettings.chat_font_family} oninput={handleChatFontFamilyChange} />
          </div>
        {:else}
          <div class="sync-info">
            Using editor font: {currentSettings.font_size}px {currentSettings.font_family}
          </div>
        {/if}
      </section>

      <section>
        <h3>LLM Provider</h3>
        <div class="field">
          <label>Default Provider</label>
          <select bind:value={selectedProvider}>
            {#each providers as provider}
              <option value={provider.id} disabled={!provider.available}>
                {provider.name} {provider.available ? '' : '(unavailable)'}
              </option>
            {/each}
            {#if providers.length === 0}
              <option value="ollama">Ollama</option>
              <option value="lmstudio">LM Studio</option>
            {/if}
          </select>
        </div>
      </section>

      <section>
        <h3>Provider URLs</h3>
        <div class="field">
          <label>llama.cpp Server URL</label>
          <input
            type="text"
            value={currentSettings.llamacpp_url}
            oninput={(e) => { if (currentSettings) currentSettings.llamacpp_url = (e.target as HTMLInputElement).value; }}
            placeholder="http://127.0.0.1:8000"
          />
          <span class="hint">OpenAI-compatible API endpoint (without /v1)</span>
        </div>
        <div class="field">
          <label>LM Studio Server URL</label>
          <input
            type="text"
            value={currentSettings.lmstudio_url}
            oninput={(e) => { if (currentSettings) currentSettings.lmstudio_url = (e.target as HTMLInputElement).value; }}
            placeholder="http://127.0.0.1:1234"
          />
          <span class="hint">OpenAI-compatible API endpoint (without /v1)</span>
        </div>
      </section>

      <section>
        <h3>Ghost Cursor</h3>
        <div class="field">
          <label>Accept Mode</label>
          <select bind:value={currentSettings.ghost_accept_mode}>
            <option value="popup">Popup near cursor</option>
            <option value="chat">Show in chat panel</option>
          </select>
        </div>
      </section>

      <section>
        <h3>System Prompt</h3>
        <div class="field">
          <div class="prompt-header">
            <label>Custom prompt for LLM</label>
            <button class="reset-btn" onclick={resetSystemPrompt} title="Reset to default">
              ↺ Reset
            </button>
          </div>
          <textarea
            class="system-prompt"
            value={currentSettings.system_prompt || ''}
            oninput={handleSystemPromptChange}
            rows="8"
          ></textarea>
          <span class="hint">
            {#if currentSettings.system_prompt && currentSettings.system_prompt !== defaultSystemPrompt}
              Using custom prompt
            {:else}
              Using default prompt (tools list is auto-appended)
            {/if}
          </span>
        </div>
      </section>

      <div class="actions">
        <button class="secondary" onclick={handleCancel}>Cancel</button>
        <button class="primary" onclick={handleSave} disabled={saving}>
          {saving ? 'Saving...' : 'Save'}
        </button>
      </div>
    </div>
  {:else}
    <p>Loading settings...</p>
  {/if}
</Modal>

<style>
  .settings-form {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  label {
    font-size: 13px;
    color: var(--text-primary);
  }

  .hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  .sync-info {
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  input, select {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 12px;
    color: var(--text-primary);
    font-size: 13px;
  }

  input:focus, select:focus {
    border-color: var(--accent);
    outline: none;
  }

  .size-controls {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .presets {
    display: flex;
    gap: 4px;
  }

  .preset {
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    width: 32px;
    height: 32px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    transition: all 0.15s;
  }

  .preset:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .preset.active {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  .stepper {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .stepper button {
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    width: 32px;
    height: 32px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 16px;
  }

  .stepper button:hover {
    background: var(--bg-hover);
  }

  .stepper span {
    min-width: 50px;
    text-align: center;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 8px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }

  .actions button {
    padding: 8px 16px;
    border-radius: 4px;
    font-size: 13px;
    cursor: pointer;
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

  .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .prompt-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .reset-btn {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 11px;
    cursor: pointer;
  }

  .reset-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .system-prompt {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 10px 12px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: 'JetBrains Mono', monospace;
    resize: vertical;
    min-height: 120px;
    line-height: 1.5;
  }

  .system-prompt:focus {
    border-color: var(--accent);
    outline: none;
  }

  .system-prompt::placeholder {
    color: var(--text-muted);
    opacity: 0.7;
  }
</style>
