/**
 * Ghost Cursor Extension for CodeMirror 6
 *
 * Renders AI-generated text as a semi-transparent "ghost" at cursor position.
 * User can accept (Tab) or reject (Escape) the suggestion.
 */

import {
  StateField,
  StateEffect,
  RangeSet,
  type Extension
} from '@codemirror/state';
import {
  EditorView,
  Decoration,
  WidgetType,
  ViewPlugin,
  keymap
} from '@codemirror/view';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { EditorState } from '@codemirror/state';
import { ghost as ghostApi, type GhostUpdate } from './tauri-ipc';

// --- State Effects ---

/** Start a ghost edit session */
export const startGhostEffect = StateEffect.define<{
  sessionId: string;
  position: number; // absolute offset in document
}>();

/** Update ghost with new text from stream */
export const updateGhostEffect = StateEffect.define<{
  delta: string;
  pendingText: string;
  done: boolean;
}>();

/** Clear ghost (accepted or rejected) */
export const clearGhostEffect = StateEffect.define<void>();

// --- Ghost State ---

interface GhostState {
  active: boolean;
  sessionId: string | null;
  position: number;        // doc offset where ghost starts
  pendingText: string;     // full ghost text
  done: boolean;           // stream complete?
}

const emptyGhost: GhostState = {
  active: false,
  sessionId: null,
  position: 0,
  pendingText: '',
  done: false,
};

/** StateField tracking ghost cursor state */
export const ghostStateField = StateField.define<GhostState>({
  create: () => emptyGhost,

  update(state, tr) {
    for (const effect of tr.effects) {
      if (effect.is(startGhostEffect)) {
        return {
          active: true,
          sessionId: effect.value.sessionId,
          position: effect.value.position,
          pendingText: '',
          done: false,
        };
      }
      if (effect.is(updateGhostEffect)) {
        if (!state.active) return state;
        return {
          ...state,
          pendingText: effect.value.pendingText,
          done: effect.value.done,
        };
      }
      if (effect.is(clearGhostEffect)) {
        return emptyGhost;
      }
    }

    // Adjust position if document changed before ghost
    if (tr.docChanged && state.active) {
      let newPos = state.position;
      tr.changes.iterChangedRanges((fromA, toA, fromB, toB) => {
        if (fromA < state.position) {
          const removed = toA - fromA;
          const inserted = toB - fromB;
          newPos = Math.max(fromB, state.position - removed + inserted);
        }
      });
      return { ...state, position: newPos };
    }

    return state;
  },
});

// --- Ghost Widget ---

/** Widget that renders ghost text inline */
class GhostWidget extends WidgetType {
  constructor(readonly text: string, readonly done: boolean) {
    super();
  }

  toDOM(): HTMLElement {
    const span = document.createElement('span');
    span.className = 'cm-ghost-text';
    span.textContent = this.text;

    // Add hint when done
    if (this.done && this.text.length > 0) {
      const hint = document.createElement('span');
      hint.className = 'cm-ghost-hint';
      hint.textContent = ' Tab to accept';
      span.appendChild(hint);
    }

    return span;
  }

  eq(other: GhostWidget): boolean {
    return this.text === other.text && this.done === other.done;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

/** StateField for ghost decorations */
const ghostDecorations = StateField.define<RangeSet<Decoration>>({
  create: () => Decoration.none,

  update(decos, tr) {
    const ghostState = tr.state.field(ghostStateField);

    if (!ghostState.active || ghostState.pendingText.length === 0) {
      return Decoration.none;
    }

    // Create widget decoration at ghost position
    const widget = Decoration.widget({
      widget: new GhostWidget(ghostState.pendingText, ghostState.done),
      side: 1, // after cursor
    });

    return Decoration.set([widget.range(ghostState.position)]);
  },

  provide: field => EditorView.decorations.from(field),
});

// --- Keymap ---

/**
 * Strip markdown code block markers from LLM response
 * Handles: ```lang\ncode\n``` or ```\ncode\n```
 */
function stripCodeBlock(text: string): string {
  const trimmed = text.trim();

  // Match ```lang\n...\n``` or ```\n...\n```
  const codeBlockRegex = /^```(?:\w+)?\s*\n([\s\S]*?)\n?```\s*$/;
  const match = trimmed.match(codeBlockRegex);

  if (match) {
    return match[1];
  }

  // Also handle case where closing ``` might have extra whitespace
  const altRegex = /^```(?:\w+)?\s*\n([\s\S]*?)```\s*$/;
  const altMatch = trimmed.match(altRegex);

  if (altMatch) {
    return altMatch[1].trimEnd();
  }

  return text;
}

/** Accept ghost edit with Tab (async implementation) */
async function acceptGhostAsync(view: EditorView, sessionId: string, position: number): Promise<void> {
  try {
    // Get final text from backend
    const rawText = await ghostApi.accept(sessionId);

    // Strip markdown code block if present
    const text = stripCodeBlock(rawText);

    // Insert text at ghost position
    view.dispatch({
      changes: { from: position, insert: text },
      effects: clearGhostEffect.of(undefined),
      selection: { anchor: position + text.length },
    });
  } catch (err) {
    console.error('Failed to accept ghost:', err);
    view.dispatch({ effects: clearGhostEffect.of(undefined) });
  }
}

/** Accept ghost edit with Tab (sync wrapper for keymap) */
function acceptGhost(view: EditorView): boolean {
  const state = view.state.field(ghostStateField);
  if (!state.active || !state.sessionId) return false;

  // Fire and forget - dispatch happens inside the async function
  acceptGhostAsync(view, state.sessionId, state.position);
  return true;
}

/** Reject ghost edit with Escape (async implementation) */
async function rejectGhostAsync(sessionId: string): Promise<void> {
  try {
    await ghostApi.reject(sessionId);
  } catch (err) {
    console.error('Failed to reject ghost:', err);
  }
}

/** Reject ghost edit with Escape (sync wrapper for keymap) */
function rejectGhost(view: EditorView): boolean {
  const state = view.state.field(ghostStateField);
  if (!state.active || !state.sessionId) return false;

  // Clear ghost immediately, notify backend async
  rejectGhostAsync(state.sessionId);
  view.dispatch({ effects: clearGhostEffect.of(undefined) });
  return true;
}

const ghostKeymap = keymap.of([
  { key: 'Tab', run: acceptGhost },
  { key: 'Escape', run: rejectGhost },
]);

// --- Theme ---

const ghostTheme = EditorView.baseTheme({
  '.cm-ghost-text': {
    color: '#a78bfa',
    opacity: '0.6',
    fontStyle: 'italic',
    pointerEvents: 'none',
  },
  '.cm-ghost-hint': {
    fontSize: '0.75em',
    color: '#6366f1',
    opacity: '0.8',
    marginLeft: '8px',
    fontStyle: 'normal',
  },
});

// --- Event Bridge ---

/** ViewPlugin that bridges Tauri events to CM6 state */
const ghostEventBridge = ViewPlugin.define(view => {
  let unlisten: UnlistenFn | null = null;

  // Listen to ghost:update events from Tauri
  listen<GhostUpdate>('ghost:update', (event) => {
    const update = event.payload;

    // Strip code block markers from pending text for cleaner preview
    const cleanPendingText = stripCodeBlock(update.pending_text);

    view.dispatch({
      effects: updateGhostEffect.of({
        delta: update.delta,
        pendingText: cleanPendingText,
        done: update.done,
      }),
    });
  }).then(fn => {
    unlisten = fn;
  });

  return {
    destroy() {
      unlisten?.();
    },
  };
});

// --- Public API ---

/**
 * Create ghost cursor extension bundle
 */
export function ghostCursor(): Extension {
  return [
    ghostStateField,
    ghostDecorations,
    ghostKeymap,
    ghostTheme,
    ghostEventBridge,
  ];
}

/**
 * Start a ghost edit session programmatically
 *
 * @param view - The EditorView
 * @param prompt - The user prompt for AI
 * @param filePath - Current file path
 * @returns Session ID
 */
export async function startGhostEdit(
  view: EditorView,
  prompt: string,
  filePath: string
): Promise<string> {
  const selection = view.state.selection.main;
  const pos = selection.head;

  // Convert offset to line/column
  const line = view.state.doc.lineAt(pos);
  const lineNumber = line.number;
  const column = pos - line.from;

  // Start session via IPC
  const sessionId = await ghostApi.startEdit({
    file_path: filePath,
    position: { line: lineNumber, column },
    prompt,
    context: {
      // Could include surrounding code context
      before: view.state.sliceDoc(Math.max(0, pos - 500), pos),
      after: view.state.sliceDoc(pos, Math.min(view.state.doc.length, pos + 500)),
    },
  });

  // Dispatch start effect
  view.dispatch({
    effects: startGhostEffect.of({
      sessionId,
      position: pos,
    }),
  });

  return sessionId;
}

/**
 * Check if ghost is currently active
 */
export function isGhostActive(state: EditorState): boolean {
  return state.field(ghostStateField).active;
}

/**
 * Get current ghost state
 */
export function getGhostState(state: EditorState): GhostState {
  return state.field(ghostStateField);
}
