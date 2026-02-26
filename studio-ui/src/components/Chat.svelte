<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { llm, events, session, settings as settingsApi, type Settings, type Provider, type ModelInfo, type ContextEstimate, type TopicSummary } from '../lib/tauri-ipc';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { invoke } from '@tauri-apps/api/core';
  import { selectedModelStore, selectedProviderStore, output } from '../lib/stores';
  import { aircpStore } from '../lib/aircp-store.svelte';

  interface Props {
    onGhostTrigger?: (prompt: string) => Promise<string | null>;
    settings?: Settings | null;
    currentFile?: string | null;
    getEditorContent?: () => string | null;
  }

  let { onGhostTrigger, settings: appSettings = null, currentFile = null, getEditorContent }: Props = $props();

  // Computed font styles from settings
  let chatFontSize = $derived(appSettings?.chat_font_size ?? 14);
  let chatFontFamily = $derived(appSettings?.chat_font_family ?? 'JetBrains Mono, monospace');

  // Attachment types
  interface Attachment {
    id: string;
    name: string;
    type: 'image' | 'pdf' | 'file';
    mimeType: string;
    data: string; // base64 encoded
    preview?: string; // data URL for preview
  }

  interface Message {
    role: 'user' | 'assistant' | 'system' | 'tool_call' | 'tool_result';
    content: string;
    toolName?: string;
    attachments?: Attachment[];
    thinking?: string;
  }

  let messages = $state<Message[]>([]);
  let input = $state('');
  let attachments = $state<Attachment[]>([]);
  let fileInputEl: HTMLInputElement;
  let isDragging = $state(false);

  // Input history (like bash/terminal)
  let inputHistory = $state<string[]>([]);
  let historyIndex = $state<number | null>(null);
  let savedInput = $state(''); // Save current input when browsing history

  // Auto-save session after each response
  async function saveSession() {
    if (messages.length === 0) return;
    try {
      await session.save(messages.map(m => ({
        role: m.role,
        content: m.content,
        toolName: m.toolName,
      })));
    } catch (e) {
      console.error('Failed to save session:', e);
    }
  }

  // Exposed for external calls
  export function clearMessages(): void {
    messages = [];
    currentResponse = '';
    currentThinking = '';
    thinkingDone = false;
    thinkingExpanded = false;
    expandedThinkings = new Set();
    streaming = false;
    topicSummaries = [];
    contextStats = null;
  }
  let streaming = $state(false);
  let currentResponse = $state('');
  let currentThinking = $state('');
  let thinkingDone = $state(false);
  let thinkingExpanded = $state(false);
  let expandedThinkings = $state<Set<number>>(new Set()); // Track which message indices have expanded thinking
  let messagesEl: HTMLDivElement;
  let unsubscribeChunk: (() => void) | null = null;
  let unsubscribeToolCall: (() => void) | null = null;
  let unsubscribeToolResult: (() => void) | null = null;
  let unsubscribeDragDrop: (() => void) | null = null;
  let unsubscribeThinking: (() => void) | null = null;
  let ghostMode = $state(false);

  // Status tracking
  let status = $state<string | null>(null);
  let statusIcon = $state<string>('');
  let toolsExecuted = $state(0);

  // Performance stats from last response
  let lastStats = $state<{ tokensPerSecond?: number; totalTokens?: number; totalMs?: number } | null>(null);
  let unsubscribeStatus: (() => void) | null = null;
  let unsubscribeStats: (() => void) | null = null;
  let unsubscribeDebug: (() => void) | null = null;

  // Live token tracking (client-side calculation)
  let streamStartTime = $state<number | null>(null);
  let tokenCount = $state(0);
  let liveTokensPerSec = $state<number | null>(null);

  // Context compression tracking
  let contextStats = $state<ContextEstimate | null>(null);
  let topicSummaries = $state<TopicSummary[]>([]);
  let showTopics = $state(false);

  // Provider & model selection (using shared stores for sync)
  let providers = $state<Provider[]>([]);
  let models = $state<ModelInfo[]>([]);
  let selectedProvider = $state<string>('ollama');
  let selectedModel = $state<string>('');
  let showProviderMenu = $state(false);
  let unsubModelStore: (() => void) | null = null;
  let unsubProviderStore: (() => void) | null = null;

  onMount(async () => {
    // Auto-restore last session
    try {
      const restored = await session.getLatest();
      if (restored && restored.length > 0) {
        messages = restored.map(m => ({
          role: m.role as Message['role'],
          content: m.content,
          toolName: m.toolName,
        }));
        await tick();
        scrollToBottom();
      }
    } catch (e) {
      console.error('Failed to auto-restore session:', e);
    }

    // Subscribe to store changes (sync with Modal)
    unsubModelStore = selectedModelStore.subscribe(value => {
      if (value && value !== selectedModel) {
        selectedModel = value;
      }
    });
    unsubProviderStore = selectedProviderStore.subscribe(async (value) => {
      if (value && value !== selectedProvider) {
        selectedProvider = value;
        // Reload models when provider changes from external source (ProviderModal)
        await loadModels(value);
      }
    });

    // Load providers and models
    await loadProviders();

    // Listen for LLM chunks
    unsubscribeChunk = await events.onLlmChunk((chunk) => {
      if (chunk.done) {
        if (currentResponse.trim()) {
          // Attach thinking to the message if present
          const assistantMessage: Message = {
            role: 'assistant',
            content: currentResponse,
            thinking: currentThinking || undefined,
          };
          messages = [...messages, assistantMessage];
        }
        currentResponse = '';
        currentThinking = '';
        thinkingDone = false;
        streaming = false;
        status = null;
        toolsExecuted = 0;
        // Reset live stats
        streamStartTime = null;
        tokenCount = 0;
        liveTokensPerSec = null;
        // Auto-save session after each complete response
        saveSession();
        // Update context stats
        updateContextStats();
      } else {
        // Start timing on first chunk
        if (!streamStartTime) {
          streamStartTime = Date.now();
          tokenCount = 0;
        }

        // Count tokens (rough: each non-empty delta ≈ 1 token)
        if (chunk.delta) {
          tokenCount++;

          // Calculate live tk/s every few tokens
          const elapsed = (Date.now() - streamStartTime) / 1000;
          if (elapsed > 0.1) {
            liveTokensPerSec = tokenCount / elapsed;
          }
        }

        // Update status based on content
        if (!currentResponse && chunk.delta) {
          status = 'Generating response...';
          statusIcon = '✨';
        }
        currentResponse += chunk.delta;
      }
      scrollToBottom();
    });

    // Listen for tool calls
    unsubscribeToolCall = await events.onToolCall((toolCall) => {
      // Finalize any streaming response first
      if (currentResponse.trim()) {
        messages = [...messages, { role: 'assistant', content: currentResponse }];
        currentResponse = '';
      }

      // Update status
      toolsExecuted++;
      status = `Executing: ${toolCall.name}`;
      statusIcon = '🔧';

      messages = [...messages, {
        role: 'tool_call',
        content: toolCall.args,
        toolName: toolCall.name,
      }];
      scrollToBottom();
    });

    // Listen for tool results
    unsubscribeToolResult = await events.onToolResult((toolResult) => {
      status = `Processing result from ${toolResult.name}...`;
      statusIcon = '📋';

      messages = [...messages, {
        role: 'tool_result',
        content: toolResult.result,
        toolName: toolResult.name,
      }];
      scrollToBottom();

      // Brief delay then show "thinking" while LLM processes
      setTimeout(() => {
        if (streaming) {
          status = 'Thinking...';
          statusIcon = '🤔';
        }
      }, 100);
    });

    // Listen for LLM thinking (streaming thinking bubble)
    unsubscribeThinking = await events.onLlmThinking((thinking) => {
      if (thinking.done) {
        // Thinking is done, mark as complete but keep visible
        thinkingDone = true;
      } else {
        // Append thinking delta
        if (!currentThinking) {
          // Starting new thinking session
          thinkingDone = false;
          thinkingExpanded = false;
        }
        currentThinking += thinking.delta;
        scrollToBottom();
      }
    });

    // Listen for LLM status updates (e.g., "Parsing tool call...")
    unsubscribeStatus = await events.onLlmStatus((statusUpdate) => {
      status = statusUpdate.message;
      statusIcon = statusUpdate.icon;
    });

    // Listen for LLM performance stats
    unsubscribeStats = await events.onLlmStats((stats) => {
      console.log('📊 Received LLM stats:', stats);
      lastStats = {
        tokensPerSecond: stats.tokens_per_second,
        totalTokens: stats.total_tokens,
        totalMs: stats.total_ms,
      };
      console.log('📊 lastStats set to:', lastStats, 'streaming:', streaming);
    });

    // Listen for LLM debug messages (output to console panel)
    unsubscribeDebug = await events.onLlmDebug((message) => {
      output.debug('LLM', message);
    });

    // Listen for Tauri drag & drop events
    unsubscribeDragDrop = await getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        isDragging = true;
      } else if (event.payload.type === 'leave') {
        isDragging = false;
      } else if (event.payload.type === 'drop') {
        isDragging = false;
        console.log('Tauri drop event:', event.payload.paths);
        await processFilePaths(event.payload.paths);
      }
    });
  });

  onDestroy(() => {
    unsubscribeChunk?.();
    unsubscribeToolCall?.();
    unsubscribeDragDrop?.();
    unsubscribeToolResult?.();
    unsubscribeThinking?.();
    unsubscribeStatus?.();
    unsubscribeDebug?.();
    unsubscribeStats?.();
    unsubModelStore?.();
    unsubProviderStore?.();
  });

  // Update context stats from current messages (uses real server context size)
  async function updateContextStats() {
    if (messages.length === 0) {
      contextStats = null;
      return;
    }
    const chatMessages = messages
      .filter(m => m.role !== 'system')
      .map(m => ({ role: m.role, content: m.content }));
    try {
      // V2 uses real n_ctx from server
      contextStats = await llm.estimateTokensV2(chatMessages);
    } catch (e) {
      console.error('Failed to estimate tokens:', e);
      // Fallback to old method
      try {
        contextStats = await llm.estimateTokens(chatMessages);
      } catch (e2) {
        console.error('Fallback also failed:', e2);
      }
    }
  }

  // Compact context manually (for /compact command)
  // force=true bypasses threshold check (useful after truncation warning)
  async function compactContext(force: boolean = false) {
    if (messages.length < 6) {
      messages = [...messages, {
        role: 'system',
        content: 'Not enough messages to compact (minimum 6 required).'
      }];
      scrollToBottom();
      return;
    }

    status = force ? 'Force compacting context...' : 'Compacting context...';
    statusIcon = '📦';

    try {
      const chatMessages = messages
        .filter(m => m.role !== 'system')
        .map(m => ({ role: m.role, content: m.content }));

      const result = await llm.compactContext(chatMessages, force);

      // Add topic summary to our list
      topicSummaries = [...topicSummaries, result.topic];

      // Replace messages with remaining ones + system notification
      const remainingMsgs: Message[] = result.remaining_messages.map(m => ({
        role: m.role as Message['role'],
        content: m.content,
      }));

      messages = [
        {
          role: 'system',
          content: `📦 Compressed ${result.compressed_count} messages into topic: "${result.topic.title}"`
        },
        ...remainingMsgs
      ];

      // Update stats
      await updateContextStats();

      status = null;
      scrollToBottom();
    } catch (e) {
      console.error('Failed to compact context:', e);
      messages = [...messages, {
        role: 'system',
        content: `Failed to compact: ${e}`
      }];
      status = null;
      scrollToBottom();
    }
  }

  // Auto-compact if approaching context limit
  async function autoCompactIfNeeded() {
    if (messages.length < 6) return;

    try {
      const chatMessages = messages
        .filter(m => m.role !== 'system')
        .map(m => ({ role: m.role, content: m.content }));

      const estimate = await llm.estimateTokens(chatMessages);

      if (estimate.needs_compression) {
        console.log('Auto-compacting context (usage:', estimate.usage_percent + '%)');
        await compactContext();
      }
    } catch (e) {
      console.error('Auto-compact check failed:', e);
    }
  }

  async function send() {
    if (!input.trim() || streaming) return;

    const userMessage = input.trim();

    // Add to input history (avoid duplicates of last entry)
    if (userMessage && (inputHistory.length === 0 || inputHistory[inputHistory.length - 1] !== userMessage)) {
      inputHistory = [...inputHistory, userMessage];
    }
    // Reset history navigation
    historyIndex = null;
    savedInput = '';

    input = '';
    // Reset textarea height
    const textarea = document.querySelector('.input-row textarea') as HTMLTextAreaElement;
    if (textarea) textarea.style.height = 'auto';

    // Check for ghost command: /ghost <prompt>
    if (userMessage.startsWith('/ghost ')) {
      const prompt = userMessage.slice(7).trim();
      if (prompt && onGhostTrigger) {
        messages = [...messages, { role: 'user', content: userMessage }];
        const sessionId = await onGhostTrigger(prompt);
        if (sessionId) {
          messages = [...messages, {
            role: 'system',
            content: `Ghost edit started. Press Tab to accept or Escape to reject.`
          }];
        } else {
          messages = [...messages, {
            role: 'system',
            content: `No file open. Please open a file first.`
          }];
        }
        scrollToBottom();
        return;
      }
    }

    // Check for slash commands
    if (userMessage === '/resume' || userMessage === '/r') {
      try {
        const restored = await session.getLatest();
        if (restored && restored.length > 0) {
          messages = restored.map(m => ({
            role: m.role as Message['role'],
            content: m.content,
            toolName: m.toolName,
          }));
          messages = [...messages, {
            role: 'system',
            content: `Session restored (${restored.length} messages)`
          }];
        } else {
          messages = [...messages, {
            role: 'system',
            content: 'No previous session found.'
          }];
        }
      } catch (e) {
        messages = [...messages, {
          role: 'system',
          content: `Failed to restore session: ${e}`
        }];
      }
      scrollToBottom();
      return;
    }

    if (userMessage === '/clear' || userMessage === '/c') {
      messages = [];
      topicSummaries = [];
      contextStats = null;
      messages = [...messages, {
        role: 'system',
        content: 'Chat cleared.'
      }];
      scrollToBottom();
      return;
    }

    if (userMessage === '/help' || userMessage === '/h') {
      messages = [...messages, {
        role: 'system',
        content: `Available commands:
/ghost - Toggle ghost mode (generate code at cursor)
/resume or /r - Restore last session
/clear or /c - Clear chat history
/compact - Compress old messages into topic summaries
/compact force - Force compression even if under threshold
/aircp <msg> - Send message to aIRCp #general
/help or /h - Show this help`
      }];
      scrollToBottom();
      return;
    }

    // Compact context command
    if (userMessage === '/compact' || userMessage === '/compact force') {
      const force = userMessage === '/compact force';
      await compactContext(force);
      return;
    }

    // aIRCp bridge command: /aircp <message>
    if (userMessage.startsWith('/aircp ')) {
      const msg = userMessage.slice(7).trim();
      if (msg) {
        aircpStore.sendMessage(msg);
        messages = [...messages, { role: 'user', content: userMessage }];
        messages = [...messages, {
          role: 'system',
          content: `Sent to aIRCp #general: "${msg}"`
        }];
      } else {
        messages = [...messages, {
          role: 'system',
          content: 'Usage: /aircp <message>'
        }];
      }
      scrollToBottom();
      return;
    }

    // Check for ghost toggle
    if (userMessage === '/ghost') {
      ghostMode = !ghostMode;
      messages = [...messages, {
        role: 'system',
        content: ghostMode
          ? 'Ghost mode ON. Messages will generate code at cursor.'
          : 'Ghost mode OFF. Normal chat mode.'
      }];
      scrollToBottom();
      return;
    }

    // Build message content - include current file context if available
    let messageContent = userMessage;
    const editorContent = getEditorContent?.();
    if (currentFile && editorContent) {
      // Get file extension for language hint
      const ext = currentFile.split('.').pop() || '';
      const langMap: Record<string, string> = {
        rs: 'rust', py: 'python', js: 'javascript', ts: 'typescript',
        svelte: 'svelte', json: 'json', md: 'markdown', c: 'c', cpp: 'cpp',
        h: 'c', go: 'go', java: 'java', html: 'html', css: 'css'
      };
      const lang = langMap[ext] || ext;

      // Truncate content if too long (keep first 2000 chars + last 500 chars)
      let content = editorContent;
      if (content.length > 3000) {
        content = content.slice(0, 2000) + '\n... [truncated] ...\n' + content.slice(-500);
      }

      messageContent = `[Current file: ${currentFile}]\n\`\`\`${lang}\n${content}\n\`\`\`\n\n${userMessage}`;
    }

    // Create user message with attachments and file context
    const userMessageObj: Message = {
      role: 'user',
      content: messageContent,
      attachments: attachments.length > 0 ? [...attachments] : undefined,
    };
    messages = [...messages, userMessageObj];

    // Clear attachments after adding to message
    const currentAttachments = [...attachments];
    attachments = [];

    // In ghost mode, trigger ghost edit instead of chat
    if (ghostMode && onGhostTrigger) {
      const sessionId = await onGhostTrigger(userMessage);
      if (sessionId) {
        messages = [...messages, {
          role: 'system',
          content: `Ghost edit started. Press Tab to accept or Escape to reject.`
        }];
      }
      scrollToBottom();
      return;
    }

    // Auto-compact if needed before sending
    await autoCompactIfNeeded();

    // Normal chat
    streaming = true;
    status = 'Sending to LLM...';
    statusIcon = '📤';
    toolsExecuted = 0;
    // Reset thinking for new message
    currentThinking = '';
    thinkingDone = false;
    thinkingExpanded = false;

    // Build history with attachments in the format expected by the backend
    const history = messages
      .filter(m => m.role !== 'system')
      .map(m => {
        const attachments = m.attachments?.map(a => ({
          name: a.name,
          mime_type: a.mimeType,
          data: a.data,
        })) || [];

        // Add hint for vision models when images are attached
        const imageCount = attachments.filter(a => a.mime_type.startsWith('image/')).length;
        const pdfCount = attachments.filter(a => a.mime_type === 'application/pdf').length;
        let content = m.content;

        if (m.role === 'user' && (imageCount > 0 || pdfCount > 0)) {
          const hints: string[] = [];
          if (imageCount > 0) hints.push(`${imageCount} image${imageCount > 1 ? 's' : ''}`);
          if (pdfCount > 0) hints.push(`${pdfCount} PDF${pdfCount > 1 ? 's' : ''}`);
          content = `[Attached: ${hints.join(', ')}] ${content}`;
        }

        return { role: m.role, content, attachments };
      });

    await llm.streamChat(history);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    } else if (e.key === 'ArrowUp') {
      // Navigate history backward
      if (inputHistory.length === 0) return;

      if (historyIndex === null) {
        // Starting to browse - save current input
        savedInput = input;
        historyIndex = inputHistory.length - 1;
      } else if (historyIndex > 0) {
        historyIndex--;
      }

      input = inputHistory[historyIndex];
      e.preventDefault();
    } else if (e.key === 'ArrowDown') {
      // Navigate history forward
      if (historyIndex === null) return;

      if (historyIndex < inputHistory.length - 1) {
        historyIndex++;
        input = inputHistory[historyIndex];
      } else {
        // Back to current input
        historyIndex = null;
        input = savedInput;
      }
      e.preventDefault();
    }
  }

  async function scrollToBottom() {
    // Wait for Svelte to update the DOM before scrolling
    await tick();
    if (messagesEl) {
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }
  }

  function toggleGhostMode() {
    ghostMode = !ghostMode;
    messages = [...messages, {
      role: 'system',
      content: ghostMode
        ? 'Ghost mode ON. Messages will generate code at cursor.'
        : 'Ghost mode OFF. Normal chat mode.'
    }];
    scrollToBottom();
  }

  // Provider & model management
  async function loadProviders() {
    try {
      providers = await llm.listProviders();

      // Try to load persisted provider from settings
      const currentSettings = await settingsApi.get();
      const persistedProvider = currentSettings.default_provider;
      const persistedModel = currentSettings.default_model;

      // Use persisted provider if it's available, otherwise use first available
      let providerToUse: string | null = null;
      let isFallback = false;

      if (persistedProvider) {
        const persistedAvailable = providers.find(p => p.id === persistedProvider && p.available);
        if (persistedAvailable) {
          providerToUse = persistedAvailable.id;
        }
      }

      // Fallback to first available provider (don't persist this!)
      if (!providerToUse) {
        const firstAvailable = providers.find(p => p.available);
        if (firstAvailable) {
          providerToUse = firstAvailable.id;
          isFallback = true;
        }
      }

      if (providerToUse) {
        selectedProvider = providerToUse;
        selectedProviderStore.set(providerToUse);

        // Only persist if it's the actual saved provider (not a fallback)
        if (!isFallback) {
          await llm.setProvider(providerToUse);
        }

        await loadModels(providerToUse, isFallback ? null : persistedModel);
      }
    } catch (e) {
      console.error('Failed to load providers:', e);
    }
  }

  async function loadModels(providerId: string, persistedModelOverride?: string | null) {
    try {
      models = await llm.listModels(providerId);
      if (models.length > 0) {
        // Use override if provided, otherwise load from settings
        let persistedModel = persistedModelOverride;
        if (persistedModel === undefined) {
          const currentSettings = await settingsApi.get();
          persistedModel = currentSettings.default_model;
        }

        // Use persisted model if it exists in the list, otherwise use first model
        const modelExists = persistedModel && models.some(m => m.name === persistedModel);
        const modelToUse = modelExists ? persistedModel! : models[0].name;

        selectedModel = modelToUse;
        selectedModelStore.set(modelToUse);

        // Only persist if we're using the persisted model (not a fallback)
        // persistedModelOverride === null means "don't persist" (fallback mode)
        if (persistedModelOverride !== null) {
          await llm.setModel(modelToUse);
        }
      }
    } catch (e) {
      console.error('Failed to load models:', e);
      models = [];
    }
  }

  async function selectProvider(providerId: string) {
    selectedProvider = providerId;
    selectedProviderStore.set(providerId);
    showProviderMenu = false;
    await llm.setProvider(providerId);
    await loadModels(providerId);
  }

  function toggleProviderMenu() {
    showProviderMenu = !showProviderMenu;
  }

  function getProviderIcon(kind: string): string {
    return kind === 'local' ? '💻' : '☁️';
  }

  // File attachment handling
  function openFilePicker() {
    fileInputEl?.click();
  }

  async function handleFileSelect(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files) {
      await processFiles(Array.from(input.files));
      input.value = ''; // Reset for re-selection
    }
  }

  async function processFiles(files: File[]) {
    console.log('processFiles called with', files.length, 'files');
    for (const file of files) {
      console.log('Processing file:', file.name, file.type);
      // Determine type
      let type: 'image' | 'pdf' | 'file' = 'file';
      if (file.type.startsWith('image/')) {
        type = 'image';
      } else if (file.type === 'application/pdf') {
        type = 'pdf';
      }

      // Read as base64
      const data = await readFileAsBase64(file);
      console.log('Base64 data length:', data.length);
      const id = crypto.randomUUID();

      const attachment: Attachment = {
        id,
        name: file.name,
        type,
        mimeType: file.type,
        data,
        preview: type === 'image' ? `data:${file.type};base64,${data}` : undefined,
      };

      attachments = [...attachments, attachment];
      console.log('Added attachment, total:', attachments.length);
    }
  }

  // Process file paths from Tauri drag & drop
  async function processFilePaths(paths: string[]) {
    for (const filePath of paths) {
      const fileName = filePath.split('/').pop() || filePath.split('\\').pop() || 'file';
      const ext = fileName.split('.').pop()?.toLowerCase() || '';

      // Determine type and mimeType from extension
      let type: 'image' | 'pdf' | 'file' = 'file';
      let mimeType = 'application/octet-stream';

      if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(ext)) {
        type = 'image';
        mimeType = ext === 'jpg' ? 'image/jpeg' : `image/${ext}`;
      } else if (ext === 'pdf') {
        type = 'pdf';
        mimeType = 'application/pdf';
      }

      try {
        // Read file using our custom Tauri command (returns base64 directly)
        const base64 = await invoke<string>('read_file_base64', { path: filePath });

        const attachment: Attachment = {
          id: crypto.randomUUID(),
          name: fileName,
          type,
          mimeType,
          data: base64,
          preview: type === 'image' ? `data:${mimeType};base64,${base64}` : undefined,
        };

        attachments = [...attachments, attachment];
        console.log('Added attachment from path:', fileName);
      } catch (e) {
        console.error('Failed to read file:', filePath, e);
      }
    }
  }

  function readFileAsBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = reader.result as string;
        // Remove data URL prefix (e.g., "data:image/png;base64,")
        const base64 = result.split(',')[1];
        resolve(base64);
      };
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
  }

  function removeAttachment(id: string) {
    attachments = attachments.filter(a => a.id !== id);
  }

  // Drag and drop handlers
  function handleDragEnter(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragging = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    // Only set to false if leaving the chat container
    const target = e.currentTarget as HTMLElement;
    if (!target?.contains(e.relatedTarget as Node)) {
      isDragging = false;
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
  }

  async function handleDrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragging = false;

    console.log('Drop event:', e.dataTransfer);
    const files = e.dataTransfer?.files;
    console.log('Files:', files);
    if (files && files.length > 0) {
      console.log('Processing', files.length, 'files');
      await processFiles(Array.from(files));
      console.log('Attachments after processing:', attachments);
    }
  }

  async function stopStream() {
    if (streaming) {
      try {
        await llm.cancelStream();
      } catch (e) {
        console.error('Failed to cancel stream:', e);
      }
      // Always reset state, even if cancel failed
      if (currentResponse) {
        messages = [...messages, { role: 'assistant', content: currentResponse + ' [stopped]' }];
        currentResponse = '';
      }
      if (currentThinking) {
        thinkingDone = true;
        currentThinking = '';
      }
      streaming = false;
      status = null;
    }
  }
</script>

<div
  class="chat"
  class:dragging={isDragging}
  style="--chat-font-size: {chatFontSize}px; --chat-font-family: {chatFontFamily};"
  role="region"
  ondragenter={(e) => { e.preventDefault(); isDragging = true; }}
  ondragleave={(e) => { e.preventDefault(); const t = e.currentTarget as HTMLElement; if (!t?.contains(e.relatedTarget as Node)) isDragging = false; }}
  ondragover={(e) => e.preventDefault()}
  ondrop={handleDrop}
>
  <!-- Hidden file input -->
  <input
    type="file"
    bind:this={fileInputEl}
    onchange={handleFileSelect}
    accept="image/*,.pdf"
    multiple
    style="display: none;"
  />

  <!-- Drag overlay -->
  {#if isDragging}
    <div
      class="drag-overlay"
      ondragover={(e) => e.preventDefault()}
      ondrop={handleDrop}
    >
      <div class="drag-content">
        <span class="drag-icon">📎</span>
        <span>Drop files here</span>
        <span class="drag-hint">Images or PDF</span>
      </div>
    </div>
  {/if}

  <div class="header">
    <span>CHAT</span>
    <div class="header-actions">
      <button
        class="header-btn"
        onclick={() => { messages = []; currentThinking = ''; expandedThinkings = new Set(); topicSummaries = []; contextStats = null; }}
        title="Clear chat"
      >
        🗑️
      </button>
      <button
        class="ghost-toggle"
        class:active={ghostMode}
        onclick={toggleGhostMode}
        title={ghostMode ? 'Ghost mode ON' : 'Ghost mode OFF'}
      >
        {ghostMode ? '👻' : '💬'}
      </button>

      <!-- Context stats badge -->
      {#if contextStats}
        <div class="context-stats" class:warning={contextStats.usage_percent > 70}>
          <button
            class="stats-btn"
            onclick={() => showTopics = !showTopics}
            title="Context: {contextStats.total_tokens}/{contextStats.max_tokens} tokens ({contextStats.usage_percent}%)"
          >
            <span class="stats-bar">
              <span class="stats-fill" style="width: {Math.min(contextStats.usage_percent, 100)}%"></span>
            </span>
            <span class="stats-text">{contextStats.total_tokens}/{contextStats.max_tokens}</span>
            {#if topicSummaries.length > 0}
              <span class="topic-count">📚{topicSummaries.length}</span>
            {/if}
          </button>

          {#if showTopics && topicSummaries.length > 0}
            <div class="topics-dropdown">
              <div class="topics-header">Compressed Topics</div>
              {#each topicSummaries as topic}
                <div class="topic-item">
                  <div class="topic-title">{topic.title}</div>
                  <div class="topic-summary">{topic.summary}</div>
                  <div class="topic-meta">{topic.original_message_count} msgs → {topic.token_count} tokens</div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Provider selector -->
      <div class="provider-selector">
        <button class="provider-btn" onclick={toggleProviderMenu}>
          {#if providers.length > 0}
            {@const current = providers.find(p => p.id === selectedProvider)}
            <span class="provider-icon">{getProviderIcon(current?.kind || 'local')}</span>
            <span class="provider-name">{current?.name || selectedProvider}</span>
            {#if selectedModel}
              <span class="model-name">/ {selectedModel.split(':')[0]}</span>
            {/if}
          {:else}
            <span class="provider-name">Loading...</span>
          {/if}
          <span class="dropdown-arrow">▾</span>
        </button>

        {#if showProviderMenu}
          <div class="provider-menu">
            <div class="menu-section">
              <div class="menu-title">Providers</div>
              {#each providers as provider}
                <button
                  class="menu-item"
                  class:active={provider.id === selectedProvider}
                  class:disabled={!provider.available}
                  onclick={() => provider.available && selectProvider(provider.id)}
                >
                  <span class="provider-icon">{getProviderIcon(provider.kind)}</span>
                  <span>{provider.name}</span>
                  {#if !provider.available}
                    <span class="status-badge offline">offline</span>
                  {:else}
                    <span class="status-badge online">●</span>
                  {/if}
                </button>
              {/each}
            </div>

            {#if models.length > 0}
              <div class="menu-section">
                <div class="menu-title">Models</div>
                {#each models as model}
                  <button
                    class="menu-item"
                    class:active={model.name === selectedModel}
                    onclick={() => { selectedModel = model.name; selectedModelStore.set(model.name); llm.setModel(model.name); showProviderMenu = false; }}
                  >
                    <span class="model-info">
                      <span>{model.name}</span>
                      <span class="model-size">{model.size}</span>
                    </span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </div>

  <div class="messages" bind:this={messagesEl}>
    {#each messages as message, idx}
      <div class="message {message.role}">
        <div class="avatar">
          {#if message.role === 'user'}👤
          {:else if message.role === 'assistant'}🤖
          {:else if message.role === 'tool_call'}🔧
          {:else if message.role === 'tool_result'}📋
          {:else}ℹ️{/if}
        </div>
        <div class="content">
          {#if message.role === 'assistant' && message.thinking}
            <div class="message-thinking">
              <button class="thinking-toggle" onclick={() => {
                const newSet = new Set(expandedThinkings);
                if (newSet.has(idx)) {
                  newSet.delete(idx);
                } else {
                  newSet.add(idx);
                }
                expandedThinkings = newSet;
              }}>
                <span class="toggle-icon">{expandedThinkings.has(idx) ? '▼' : '▶'}</span>
                <span class="thinking-label">Thought ({message.thinking.length} chars)</span>
              </button>
              {#if expandedThinkings.has(idx)}
                <div class="thinking-text">{message.thinking}</div>
              {/if}
            </div>
          {/if}
          {#if message.role === 'tool_call'}
            <div class="tool-header">
              <span class="tool-badge">TOOL</span>
              <strong>{message.toolName}</strong>
            </div>
            <details class="tool-details">
              <summary>Arguments</summary>
              <pre class="tool-args">{message.content}</pre>
            </details>
          {:else if message.role === 'tool_result'}
            <div class="tool-header">
              <span class="tool-badge result">RESULT</span>
              <strong>{message.toolName}</strong>
              <span class="result-size">{Math.round(message.content.length / 1024 * 10) / 10} KB</span>
            </div>
            <details class="tool-details" open={message.content.length < 300}>
              <summary>Output ({message.content.split('\n').length} lines)</summary>
              <pre class="tool-result">{message.content.slice(0, 2000)}{message.content.length > 2000 ? '\n... (truncated)' : ''}</pre>
            </details>
          {:else}
            <!-- Show attachments if any -->
            {#if message.attachments && message.attachments.length > 0}
              <div class="message-attachments">
                {#each message.attachments as att}
                  {#if att.type === 'image' && att.preview}
                    <div class="attachment-thumb">
                      <img src={att.preview} alt={att.name} />
                      <span class="attachment-name">{att.name}</span>
                    </div>
                  {:else if att.type === 'pdf'}
                    <div class="attachment-file pdf">
                      <span class="file-icon">📄</span>
                      <span class="attachment-name">{att.name}</span>
                    </div>
                  {:else}
                    <div class="attachment-file">
                      <span class="file-icon">📁</span>
                      <span class="attachment-name">{att.name}</span>
                    </div>
                  {/if}
                {/each}
              </div>
            {/if}
            {message.content}
          {/if}
        </div>
      </div>
    {/each}

    {#if streaming && currentThinking}
      <div class="message thinking" class:done={thinkingDone}>
        <div class="avatar">💭</div>
        <div class="content thinking-content">
          <button class="thinking-toggle" onclick={() => thinkingExpanded = !thinkingExpanded}>
            <span class="toggle-icon">{thinkingExpanded ? '▼' : '▶'}</span>
            <span class="thinking-label">
              {#if !thinkingDone}
                Thinking...
              {:else}
                Thought ({currentThinking.length} chars)
              {/if}
            </span>
            {#if !thinkingDone}
              <span class="thinking-spinner"></span>
            {/if}
          </button>
          {#if thinkingExpanded}
            <div class="thinking-text">{currentThinking}</div>
          {/if}
        </div>
      </div>
    {/if}

    {#if streaming && currentResponse}
      <div class="message assistant">
        <div class="avatar">🤖</div>
        <div class="content">{currentResponse}<span class="cursor">█</span></div>
      </div>
    {/if}
  </div>

  <!-- Status bar -->
  {#if status}
    <div class="status-bar">
      <span class="status-icon">{statusIcon}</span>
      <span class="status-text">{status}</span>
      {#if liveTokensPerSec && liveTokensPerSec > 0}
        <span class="live-stats">⚡ {liveTokensPerSec.toFixed(1)} tk/s</span>
        <span class="live-stats">📝 {tokenCount} tokens</span>
      {/if}
      {#if toolsExecuted > 0}
        <span class="tools-count">{toolsExecuted} tool{toolsExecuted > 1 ? 's' : ''}</span>
      {/if}
      <div class="status-spinner"></div>
    </div>
  {:else if lastStats && !streaming && (lastStats.tokensPerSecond || lastStats.totalTokens || lastStats.totalMs)}
    <!-- Performance stats from last response -->
    <div class="perf-stats-bar">
      {#if lastStats.tokensPerSecond}
        <span class="stat">⚡ {lastStats.tokensPerSecond.toFixed(1)} tok/s</span>
      {/if}
      {#if lastStats.totalTokens}
        <span class="stat">📊 {lastStats.totalTokens} tokens</span>
      {/if}
      {#if lastStats.totalMs}
        <span class="stat">⏱ {(lastStats.totalMs / 1000).toFixed(1)}s</span>
      {/if}
    </div>
  {/if}

  <div class="input-area">
    <!-- Pending attachments preview -->
    {#if attachments.length > 0}
      <div class="pending-attachments">
        {#each attachments as att}
          <div class="pending-attachment" title={att.name}>
            {#if att.type === 'image' && att.preview}
              <img src={att.preview} alt={att.name} />
            {:else if att.type === 'pdf'}
              <span class="pending-icon">📄</span>
            {:else}
              <span class="pending-icon">📁</span>
            {/if}
            <button
              class="remove-attachment"
              onclick={() => removeAttachment(att.id)}
              title="Remove"
            >×</button>
          </div>
        {/each}
      </div>
    {/if}

    <div class="input-row">
      <button class="attach" onclick={openFilePicker} title="Attach file (images, PDF)">📎</button>
      <textarea
        placeholder={ghostMode ? "Describe code to generate at cursor..." : "Ask about your code... (Shift+Enter for new line)"}
        bind:value={input}
        onkeydown={handleKeydown}
        oninput={(e) => {
          // Auto-resize textarea
          const target = e.target as HTMLTextAreaElement;
          target.style.height = 'auto';
          target.style.height = Math.min(target.scrollHeight, 200) + 'px';
        }}
        disabled={streaming}
        rows="1"
      ></textarea>
      {#if streaming}
        <button class="stop" onclick={stopStream} title="Stop generation">⏹</button>
      {:else}
        <button class="send" onclick={send} disabled={!input.trim() && attachments.length === 0}>
          {#if ghostMode}👻{:else}↵{/if}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .chat {
    height: 100%;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    position: relative;
    overflow: hidden; /* Contain children within bounds */
  }

  .chat.dragging {
    border: 2px dashed var(--accent);
  }

  /* Drag overlay */
  .drag-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    backdrop-filter: blur(4px);
  }

  .drag-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    color: var(--text-primary);
    font-size: 16px;
  }

  .drag-icon {
    font-size: 48px;
  }

  .drag-hint {
    font-size: 12px;
    color: var(--text-muted);
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    font-size: var(--font-size-sm);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-btn {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 6px;
    cursor: pointer;
    font-size: 12px;
    opacity: 0.7;
  }

  .header-btn:hover {
    opacity: 1;
    background: var(--bg-hover);
  }

  .ghost-toggle {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 6px;
    cursor: pointer;
    font-size: 14px;
  }

  .ghost-toggle.active {
    background: var(--accent);
    border-color: var(--accent);
  }

  /* Provider selector */
  .provider-selector {
    position: relative;
  }

  .provider-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .provider-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .provider-icon {
    font-size: 12px;
  }

  .provider-name {
    font-weight: 500;
  }

  .model-name {
    color: var(--text-muted);
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-arrow {
    font-size: 10px;
    color: var(--text-muted);
  }

  .provider-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    min-width: 200px;
    z-index: 100;
    overflow: hidden;
  }

  .menu-section {
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
  }

  .menu-section:last-child {
    border-bottom: none;
  }

  .menu-title {
    padding: 4px 12px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-muted);
    letter-spacing: 0.5px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
  }

  .menu-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .menu-item.active {
    background: var(--accent);
    color: white;
  }

  .menu-item.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .status-badge {
    margin-left: auto;
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 3px;
  }

  .status-badge.online {
    color: #4ec9b0;
  }

  .status-badge.offline {
    background: rgba(255, 255, 255, 0.1);
    color: var(--text-muted);
  }

  .model-info {
    display: flex;
    justify-content: space-between;
    width: 100%;
  }

  .model-size {
    color: var(--text-muted);
    font-size: 10px;
  }

  .messages {
    flex: 1;
    min-height: 0; /* Critical for flexbox shrinking */
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .message {
    display: flex;
    gap: 10px;
    font-size: var(--chat-font-size, 14px);
    font-family: var(--chat-font-family, 'JetBrains Mono', monospace);
  }

  .avatar {
    font-size: calc(var(--chat-font-size, 14px) + 2px);
  }

  .content {
    flex: 1;
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .message.user .content {
    background: var(--chat-user-bg);
    padding: 8px 12px;
    border-radius: 8px;
  }

  /* Message attachments */
  .message-attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
  }

  .attachment-thumb {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 4px;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 6px;
  }

  .attachment-thumb img {
    max-width: 150px;
    max-height: 100px;
    border-radius: 4px;
    object-fit: cover;
  }

  .attachment-file {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 6px;
    font-size: 12px;
  }

  .attachment-file.pdf {
    background: rgba(220, 53, 69, 0.2);
  }

  .file-icon {
    font-size: 16px;
  }

  .attachment-name {
    font-size: 10px;
    color: var(--text-muted);
    max-width: 100px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .message.system .content {
    color: var(--text-muted);
    font-style: italic;
    font-size: 12px;
  }

  .message.tool_call .content,
  .message.tool_result .content {
    background: var(--bg-tertiary, #2a2a2a);
    padding: 8px 12px;
    border-radius: 6px;
    border-left: 3px solid var(--accent, #4a9eff);
  }

  .message.tool_result .content {
    border-left-color: #4ec9b0;
  }

  .tool-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 6px;
  }

  .tool-header strong {
    color: var(--text-primary);
  }

  .tool-badge {
    background: var(--accent, #4a9eff);
    color: white;
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.5px;
  }

  .tool-badge.result {
    background: #4ec9b0;
  }

  .result-size {
    margin-left: auto;
    font-size: 10px;
    opacity: 0.7;
  }

  .tool-details {
    margin-top: 4px;
  }

  .tool-details summary {
    cursor: pointer;
    font-size: 11px;
    color: var(--text-muted);
    padding: 4px 0;
    user-select: none;
  }

  .tool-details summary:hover {
    color: var(--text-primary);
  }

  .tool-args,
  .tool-result {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11px;
    margin: 4px 0 0 0;
    padding: 8px;
    background: rgba(0, 0, 0, 0.3);
    border-radius: 4px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 200px;
    overflow-y: auto;
  }

  /* Status bar */
  .status-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: linear-gradient(90deg, var(--accent, #4a9eff) 0%, var(--bg-tertiary, #2a2a2a) 100%);
    background-size: 200% 100%;
    animation: shimmer 2s ease-in-out infinite;
    font-size: 12px;
    color: var(--text-primary);
    border-radius: 0;
  }

  @keyframes shimmer {
    0%, 100% { background-position: 200% 0; }
    50% { background-position: 0 0; }
  }

  .status-icon {
    font-size: 14px;
  }

  .status-text {
    flex: 1;
    font-weight: 500;
  }

  .tools-count {
    background: rgba(255, 255, 255, 0.15);
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 10px;
    font-weight: 600;
  }

  .live-stats {
    background: rgba(78, 201, 176, 0.3);
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 10px;
    font-weight: 600;
    color: #4ec9b0;
  }

  .status-spinner {
    width: 12px;
    height: 12px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: white;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Performance stats bar (shown after response) */
  .perf-stats-bar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 4px 12px;
    background: var(--bg-tertiary, #2a2a2a);
    font-size: 11px;
    color: var(--text-muted, #888);
    border-top: 1px solid var(--border, #333);
  }

  .perf-stats-bar .stat {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .cursor {
    animation: blink 1s infinite;
    color: var(--accent);
  }

  @keyframes blink {
    50% { opacity: 0; }
  }

  .input-area {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-top: 1px solid var(--border);
    flex-shrink: 0; /* Never shrink, always visible */
  }

  .input-row {
    display: flex;
    gap: 8px;
  }

  /* Pending attachments */
  .pending-attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-bottom: 4px;
  }

  .pending-attachment {
    position: relative;
    width: 48px;
    height: 48px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
  }

  .pending-attachment img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .pending-icon {
    font-size: 24px;
  }

  .remove-attachment {
    position: absolute;
    top: -4px;
    right: -4px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #e74c3c;
    border: none;
    color: white;
    font-size: 12px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
    padding: 0;
  }

  .remove-attachment:hover {
    background: #c0392b;
  }

  .attach, .send, .stop {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 8px;
    font-size: 16px;
  }

  .attach:hover, .send:hover {
    color: var(--text-primary);
  }

  .stop {
    color: #e74c3c;
  }

  .stop:hover {
    color: #c0392b;
    background: rgba(231, 76, 60, 0.1);
    border-radius: 4px;
  }

  .send:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  textarea {
    flex: 1;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 12px;
    color: var(--text-primary);
    font-size: var(--chat-font-size, 14px);
    font-family: var(--chat-font-family, 'JetBrains Mono', monospace);
    resize: none;
    overflow-y: auto;
    min-height: 36px;
    max-height: 200px;
    line-height: 1.4;
  }

  textarea:focus {
    border-color: var(--border-focus);
    outline: none;
  }

  textarea::placeholder {
    color: var(--text-muted);
  }

  /* Thinking bubble (streaming) */
  .message.thinking .content {
    background: var(--bg-tertiary, #2a2a2a);
    padding: 6px 10px;
    border-radius: 6px;
    border-left: 3px solid #9b59b6;
  }

  .message.thinking.done .content {
    opacity: 0.7;
  }

  /* Thinking in persisted messages */
  .message-thinking {
    background: var(--bg-tertiary, #2a2a2a);
    padding: 6px 10px;
    border-radius: 6px;
    border-left: 3px solid #9b59b6;
    margin-bottom: 8px;
    opacity: 0.8;
  }

  .thinking-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 12px;
    color: #9b59b6;
    padding: 2px 0;
    width: 100%;
    text-align: left;
  }

  .thinking-toggle:hover {
    color: #8e44ad;
  }

  .toggle-icon {
    font-size: 10px;
    opacity: 0.7;
  }

  .thinking-label {
    font-weight: 500;
  }

  .thinking-spinner {
    width: 10px;
    height: 10px;
    border: 2px solid rgba(155, 89, 182, 0.3);
    border-top-color: #9b59b6;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin-left: auto;
  }

  .thinking-text {
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid rgba(155, 89, 182, 0.2);
    font-size: 12px;
    color: var(--text-muted);
    white-space: pre-wrap;
    max-height: 300px;
    overflow-y: auto;
    line-height: 1.5;
  }

  /* Context stats badge */
  .context-stats {
    position: relative;
  }

  .stats-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 10px;
    color: var(--text-secondary);
  }

  .stats-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .context-stats.warning .stats-btn {
    border-color: #f39c12;
    color: #f39c12;
  }

  .stats-bar {
    width: 40px;
    height: 6px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 3px;
    overflow: hidden;
  }

  .stats-fill {
    display: block;
    height: 100%;
    background: #4ec9b0;
    border-radius: 3px;
    transition: width 0.3s ease;
  }

  .context-stats.warning .stats-fill {
    background: #f39c12;
  }

  .stats-text {
    font-weight: 600;
    min-width: 28px;
  }

  .topic-count {
    font-size: 10px;
    opacity: 0.8;
  }

  .topics-dropdown {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    min-width: 280px;
    max-width: 400px;
    max-height: 300px;
    overflow-y: auto;
    z-index: 100;
  }

  .topics-header {
    padding: 8px 12px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-muted);
    letter-spacing: 0.5px;
    border-bottom: 1px solid var(--border);
  }

  .topic-item {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }

  .topic-item:last-child {
    border-bottom: none;
  }

  .topic-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .topic-summary {
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.4;
    white-space: pre-wrap;
  }

  .topic-meta {
    font-size: 10px;
    color: var(--text-muted);
    margin-top: 6px;
  }
</style>
