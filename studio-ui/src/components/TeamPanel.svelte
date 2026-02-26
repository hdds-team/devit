<script lang="ts">
  import { aircpStore, type Agent, type ChatMessage, type Workflow } from '../lib/aircp-store.svelte';

  let inputValue = $state('');

  function handleSend() {
    const msg = inputValue.trim();
    if (!msg) return;
    aircpStore.sendMessage(msg);
    inputValue = '';
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function healthDot(health: string): string {
    if (health === 'online') return '#4caf50';
    if (health === 'away') return '#ffc107';
    return '#666';
  }

  function timeAgo(ts: number): string {
    if (!ts) return 'never';
    const s = Math.floor((Date.now() - ts) / 1000);
    if (s < 60) return `${s}s`;
    if (s < 3600) return `${Math.floor(s / 60)}m`;
    if (s < 86400) return `${Math.floor(s / 3600)}h`;
    return `${Math.floor(s / 86400)}d`;
  }

  function formatTime(d: Date): string {
    return d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  }

  // Phase progress
  const PHASES = ['request', 'brainstorm', 'code', 'review', 'done'];

  function phaseIndex(phase: string): number {
    const idx = PHASES.indexOf(phase);
    return idx >= 0 ? idx : 0;
  }
</script>

<div class="team-panel">
  <!-- Agents Section -->
  <section class="section">
    <h3 class="section-title">
      Agents
      <span class="count">{aircpStore.agentsUp}/{aircpStore.agentsTotal}</span>
    </h3>
    <div class="agent-list">
      {#each aircpStore.agentList as agent}
        <div class="agent-row">
          <span class="dot" style="background: {healthDot(agent.health)}"></span>
          <span class="agent-name" style="color: {agent.color}">{agent.id}</span>
          <span class="agent-role">{agent.role}</span>
          <span class="agent-time">{timeAgo(agent.lastSeen)}</span>
        </div>
      {/each}
    </div>
  </section>

  <!-- Workflow Section -->
  <section class="section">
    <h3 class="section-title">Workflow</h3>
    {#if aircpStore.workflow}
      {@const wf = aircpStore.workflow}
      <div class="workflow-card">
        <div class="wf-feature">{wf.feature}</div>
        <div class="wf-meta">
          <span>Lead: <strong>{wf.lead}</strong></span>
          <span class="wf-phase">{wf.phase}</span>
        </div>
        <div class="wf-progress-bar">
          <div
            class="wf-progress-fill"
            style="width: {((phaseIndex(wf.phase) + 1) / PHASES.length) * 100}%"
          ></div>
        </div>
      </div>
    {:else}
      <div class="empty">No active workflow</div>
    {/if}
  </section>

  <!-- Feed Section -->
  <section class="section feed-section">
    <h3 class="section-title">Feed #general</h3>
    <div class="feed">
      {#if !aircpStore.online}
        <div class="offline-badge">Offline</div>
      {/if}
      {#each aircpStore.messages as msg}
        <div class="feed-msg">
          <span class="feed-time">{formatTime(msg.timestamp)}</span>
          <span class="feed-from" style="color: {aircpStore.getAgentColor(msg.from)}">{msg.from}</span>
          <span class="feed-content">{msg.content}</span>
        </div>
      {/each}
    </div>
    <div class="feed-input">
      <input
        type="text"
        placeholder="Message as @operator..."
        bind:value={inputValue}
        onkeydown={handleKeydown}
      />
    </div>
  </section>
</div>

<style>
  .team-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    font-size: 12px;
    color: var(--text-primary, #ccc);
  }

  .section {
    padding: 8px 10px;
    border-bottom: 1px solid var(--border, #3c3c3c);
  }

  .section-title {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-secondary, #969696);
    margin: 0 0 6px 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }

  .count {
    font-size: 10px;
    color: var(--text-secondary);
    font-weight: 400;
  }

  .agent-list {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .agent-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    font-size: 11px;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .agent-name {
    font-weight: 500;
    white-space: nowrap;
  }

  .agent-role {
    color: var(--text-secondary, #666);
    font-size: 10px;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .agent-time {
    color: var(--text-secondary, #666);
    font-size: 10px;
    white-space: nowrap;
  }

  /* Workflow */
  .workflow-card {
    background: var(--bg-tertiary, #1e1e1e);
    border-radius: 4px;
    padding: 8px;
  }

  .wf-feature {
    font-weight: 600;
    margin-bottom: 4px;
  }

  .wf-meta {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: var(--text-secondary);
    margin-bottom: 6px;
  }

  .wf-phase {
    background: var(--accent, #007acc);
    color: #fff;
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .wf-progress-bar {
    height: 3px;
    background: var(--border, #3c3c3c);
    border-radius: 2px;
    overflow: hidden;
  }

  .wf-progress-fill {
    height: 100%;
    background: var(--accent, #007acc);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .empty {
    color: var(--text-secondary, #666);
    font-style: italic;
    font-size: 11px;
  }

  /* Feed */
  .feed-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-bottom: none;
  }

  .feed {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-bottom: 4px;
  }

  .feed-msg {
    font-size: 11px;
    line-height: 1.4;
    word-break: break-word;
  }

  .feed-time {
    color: var(--text-secondary, #555);
    font-size: 10px;
    margin-right: 4px;
  }

  .feed-from {
    font-weight: 600;
    margin-right: 4px;
  }

  .feed-content {
    color: var(--text-primary, #ccc);
  }

  .feed-input {
    padding-top: 4px;
    flex-shrink: 0;
  }

  .feed-input input {
    width: 100%;
    background: var(--bg-tertiary, #1e1e1e);
    border: 1px solid var(--border, #3c3c3c);
    color: var(--text-primary, #ccc);
    padding: 4px 8px;
    font-size: 11px;
    border-radius: 3px;
    outline: none;
    box-sizing: border-box;
  }

  .feed-input input:focus {
    border-color: var(--accent, #007acc);
  }

  .offline-badge {
    text-align: center;
    color: #f44336;
    font-size: 10px;
    font-weight: 600;
    padding: 4px;
    background: rgba(244, 67, 54, 0.1);
    border-radius: 3px;
    margin-bottom: 4px;
  }
</style>
