// The grey lines apprentice suggests under a line, from the `sorcerer/ghost` notification, and
// how the person's edits move or take them away. No editor code, so node's test runner covers
// it; ghosts.ts draws what it returns.

// One ghost: the document's uri, the line it hangs under (from 0) and the suggested lines.
export interface Ghost {
  uri: string;
  line: number;
  lines: string[];
}

// One change of a document, as VS Code reports it: the range it replaced (lines from 0) and
// the text put there. All the changes of one event are measured in the text before the event.
export interface LineChange {
  startLine: number;
  endLine: number;
  endCharacter: number;
  text: string;
}

// The most of a ghost shown after its line; the whole of it is on hover.
const SHOWN_AT_MOST = 120;

// parseGhost reads the notification's params, `{ uri, line, lines }`; an empty `lines` (or none)
// takes the ghost on that line away. Anything malformed is undefined (nothing changes).
//
// Example: `{ uri: 'file:///a.py', line: 4, lines: ['    return 0'] }` is a ghost under the
// fifth line.
export function parseGhost(params: unknown): Ghost | undefined {
  const ghost = params as Partial<Ghost> | null | undefined;
  if (typeof ghost?.uri !== 'string' || typeof ghost.line !== 'number' || !Number.isInteger(ghost.line) || ghost.line < 0) {
    return undefined;
  }
  const lines = Array.isArray(ghost.lines) ? ghost.lines.filter((line): line is string => typeof line === 'string') : [];
  return { uri: ghost.uri, line: ghost.line, lines };
}

// withGhost is the ghosts of a document once `ghost` is shown: it replaces the one on its line,
// and with no lines it only takes that one away.
export function withGhost(ghosts: readonly Ghost[], ghost: Ghost): Ghost[] {
  const others = ghosts.filter(known => !(known.uri === ghost.uri && known.line === ghost.line));
  return ghost.lines.length > 0 ? [...others, ghost] : others;
}

// afterChanges is the ghosts of one document after the person's edit: a ghost whose line the
// edit touched is gone (as in Neovim, the next edit of that line takes it away), and one below
// the edit moves by the lines the edit added or took out.
//
// Example: a ghost under line 4; typing on line 4 takes it away; pressing Enter on line 1 puts
// it under line 5; deleting lines 1 and 2 whole (the range 1:0 to 3:0) puts it under line 2.
export function afterChanges(ghosts: readonly Ghost[], changes: readonly LineChange[]): Ghost[] {
  return ghosts
    .filter(ghost => !changes.some(change => touches(change, ghost.line)))
    .map(ghost => ({ ...ghost, line: ghost.line + linesAddedAbove(changes, ghost.line) }));
}

// A change ending at the very start of the ghost's line, from a line above, replaced whole
// lines above it and leaves the line itself as it was.
function touches(change: LineChange, line: number): boolean {
  const endsAtItsStart = change.endLine === line && change.endCharacter === 0 && change.startLine < line;
  return change.startLine <= line && line <= change.endLine && !endsAtItsStart;
}

function linesAddedAbove(changes: readonly LineChange[], line: number): number {
  return changes
    .filter(change => change.endLine <= line && !touches(change, line))
    .reduce((sum, change) => sum + newlines(change.text) - (change.endLine - change.startLine), 0);
}

function newlines(text: string): number {
  return text.split('\n').length - 1;
}

// ghostText is what is written after the ghost's line: an arrow, then the lines without their
// indentation, joined by ⏎, at most 120 characters.
//
// Example: `['    if late:', '        raise TooLate()']` is `⤷ if late: ⏎ raise TooLate()`.
export function ghostText(lines: readonly string[]): string {
  const text = `⤷ ${lines.map(line => line.trim()).join(' ⏎ ')}`;
  return text.length > SHOWN_AT_MOST ? `${text.slice(0, SHOWN_AT_MOST - 1)}…` : text;
}

// ghostsIn is the ghosts of one document.
export function ghostsIn(ghosts: readonly Ghost[], uri: string): Ghost[] {
  return ghosts.filter(ghost => ghost.uri === uri);
}
