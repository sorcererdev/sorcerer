// The last change's pure part (no editor code, so node's test runner covers it): where an edit
// the server landed starts, and which of its marks is nearest. undoLast.ts runs it. The same
// file is in the conjure, lackey and spellbook extensions: each remembers its own server's last
// change, and the `sorcerer.lastChange` context (shared by all of them) says whose is the latest,
// so `ctrl+m u` runs the undo of the tool that changed the code last.

// LAST_CHANGE is the context key naming the tool whose change is the latest (`conjure`,
// `lackey` or `spellbook`); unset, no tool has changed anything this session.
export const LAST_CHANGE = 'sorcerer.lastChange';

// Where an edit starts in one file: its URI as the server sent it and the line (from 0) of its
// first change.
export interface Place {
  uri: string;
  line: number;
}

interface TextEditLike {
  range: { start: { line: number } };
}

// The parts of an LSP WorkspaceEdit this file reads.
export interface WorkspaceEditLike {
  changes?: Record<string, readonly TextEditLike[]>;
  documentChanges?: readonly ({ textDocument?: { uri: string }; edits?: readonly TextEditLike[]; kind?: string; uri?: string })[];
}

// placesOf is each file an edit touches, once, in the order the edit names them, with the line of
// its first change; a file the edit creates starts at line 0.
//
// Example: an edit of lines 4 and 2 of a.py and a created b.py gives
// `[{ uri: 'file:///a.py', line: 2 }, { uri: 'file:///b.py', line: 0 }]`.
export function placesOf(edit: WorkspaceEditLike | undefined): Place[] {
  const first = new Map<string, number>();
  const note = (uri: string | undefined, line: number) => {
    if (uri === undefined) return;
    const had = first.get(uri);
    if (had === undefined || line < had) first.set(uri, line);
  };
  for (const [uri, edits] of Object.entries(edit?.changes ?? {})) {
    for (const e of edits) note(uri, e.range.start.line);
  }
  for (const change of edit?.documentChanges ?? []) {
    if (change.textDocument) for (const e of change.edits ?? []) note(change.textDocument.uri, e.range.start.line);
    else if (change.kind === 'create') note(change.uri, 0);
  }
  return [...first].map(([uri, line]) => ({ uri, line }));
}

// nearestLine is the line among `lines` closest to `line`, or undefined when there are none.
//
// Example: `nearestLine([3, 40], 10)` is 3.
export function nearestLine(lines: readonly number[], line: number): number | undefined {
  let best: number | undefined;
  for (const candidate of lines) {
    if (best === undefined || Math.abs(candidate - line) < Math.abs(best - line)) best = candidate;
  }
  return best;
}

// undoneText is what is said when a tool's change is undone: `undo: conjure's last change (a.py:12)`.
export function undoneText(tool: string, file: string, line: number): string {
  return `undo: ${tool}'s last change (${file}:${line + 1})`;
}

// NOTHING_TO_UNDO is what is said when no tool has changed anything this session.
export const NOTHING_TO_UNDO = 'nothing to undo: conjure, lackey and spellbook have changed nothing yet';
