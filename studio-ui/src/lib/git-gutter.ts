/**
 * Git gutter extension for CodeMirror 6
 * Shows added/modified/deleted line indicators in the editor gutter
 */

import { gutter, GutterMarker } from '@codemirror/view';
import { StateField, StateEffect, RangeSet, RangeSetBuilder } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';
import type { GitDiffLine } from './tauri-ipc';

// Marker classes for different change types
class AddedMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div');
    el.className = 'git-gutter-added';
    return el;
  }
}

class ModifiedMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div');
    el.className = 'git-gutter-modified';
    return el;
  }
}

class DeletedMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div');
    el.className = 'git-gutter-deleted';
    return el;
  }
}

const addedMarker = new AddedMarker();
const modifiedMarker = new ModifiedMarker();
const deletedMarker = new DeletedMarker();

// State effect to update git changes
export const setGitChanges = StateEffect.define<GitDiffLine[]>();

// State field to store git change markers
const gitChangesField = StateField.define<RangeSet<GutterMarker>>({
  create() {
    return RangeSet.empty;
  },
  update(markers, tr) {
    // Look for setGitChanges effects
    for (const effect of tr.effects) {
      if (effect.is(setGitChanges)) {
        const changes = effect.value;
        const builder = new RangeSetBuilder<GutterMarker>();

        // Sort changes by line number
        const sorted = [...changes].sort((a, b) => a.line - b.line);

        for (const change of sorted) {
          // Convert 1-based line to 0-based and get line info
          const lineNum = Math.min(change.line, tr.state.doc.lines);
          if (lineNum < 1) continue;

          try {
            const line = tr.state.doc.line(lineNum);
            const marker =
              change.kind === 'added' ? addedMarker :
              change.kind === 'modified' ? modifiedMarker :
              deletedMarker;
            builder.add(line.from, line.from, marker);
          } catch {
            // Line doesn't exist, skip
          }
        }

        return builder.finish();
      }
    }

    // If document changed, try to map existing markers
    if (tr.docChanged) {
      return markers.map(tr.changes);
    }

    return markers;
  }
});

// Spacer marker to ensure gutter has consistent width
class SpacerMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div');
    el.className = 'git-gutter-spacer';
    return el;
  }
}

const spacerMarker = new SpacerMarker();

// Gutter extension
const gitGutterExtension = gutter({
  class: 'git-gutter',
  markers: (view) => view.state.field(gitChangesField),
  initialSpacer: () => spacerMarker
});

// CSS styles
const gitGutterStyles = `
  .git-gutter {
    width: 4px;
    margin-right: 2px;
  }

  .git-gutter-added {
    width: 4px;
    height: 100%;
    background-color: #28a745;
    border-radius: 2px;
  }

  .git-gutter-modified {
    width: 4px;
    height: 100%;
    background-color: #ffc107;
    border-radius: 2px;
  }

  .git-gutter-deleted {
    width: 4px;
    height: 100%;
    background-color: #dc3545;
    border-radius: 0;
    position: relative;
  }

  .git-gutter-deleted::before {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    width: 0;
    height: 0;
    border-left: 4px solid #dc3545;
    border-top: 4px solid transparent;
    border-bottom: 4px solid transparent;
  }

  .git-gutter-spacer {
    width: 4px;
  }
`;

// Inject styles
let stylesInjected = false;
function injectStyles() {
  if (stylesInjected) return;
  const styleEl = document.createElement('style');
  styleEl.textContent = gitGutterStyles;
  document.head.appendChild(styleEl);
  stylesInjected = true;
}

/**
 * Create git gutter extension for CodeMirror
 */
export function gitGutter() {
  injectStyles();
  return [gitChangesField, gitGutterExtension];
}

/**
 * Update git changes in the editor
 */
export function updateGitChanges(view: EditorView, changes: GitDiffLine[]) {
  view.dispatch({
    effects: setGitChanges.of(changes)
  });
}
