// After Spellbook: New: the opened file's holes as one snippet, and the words of the hole
// session (no editor code, so node's test runner covers it); holeSession.ts runs the session.
import type { Stop } from './newFlow';

export type { Stop };

// A position in a document: a line from 0 and a UTF-16 column (a JavaScript string index).
export interface Position {
  line: number;
  character: number;
}

// The holes of the file the session runs in, and how many holes each other file has.
export interface StopsByFile {
  mine: Stop[];
  others: Map<string, number>;
}

// The snippet that turns a stretch of lines into the session: the first and last line it
// replaces, whole, and the snippet text.
export interface HoleSnippet {
  region: { startLine: number; endLine: number };
  snippet: string;
}

// endText is the line said once the files are written: how many, how many holes are left to
// fill, and how to go on: conjure the first when the Conjure extension is here, else the marks
// in Problems.
// Example: `spellbook: wrote 2 files · 3 holes · ctrl+m c to conjure the first`.
export function endText(written: number, holes: number, conjureActive: boolean): string {
  const wrote = `spellbook: wrote ${counted(written, 'file')}`;
  if (holes === 0) return wrote;
  return conjureActive
    ? `${wrote} · ${counted(holes, 'hole')} · ctrl+m c to conjure the first`
    : `${wrote} · ${counted(holes, 'hole')}, marked (Problems)`;
}

// stopsInFile splits the apply reply's stops: those of the first stop's file, in order (the
// session runs there), and a count of the rest by file name (the end line names them).
// Example: two stops in cancel_table.py and one in cancel_table__test.py give mine = the two,
// others = { cancel_table__test.py: 1 }.
export function stopsInFile(stops: readonly Stop[]): StopsByFile {
  const file = stops[0]?.file;
  const mine = stops.filter(stop => stop.file === file);
  const others = new Map<string, number>();
  for (const stop of stops.filter(stop => stop.file !== file)) {
    const name = stop.file.split('/').at(-1) ?? stop.file;
    others.set(name, (others.get(name) ?? 0) + 1);
  }
  return { mine, others };
}

// escapeSnippet makes text literal inside a snippet: `\`, `$` and `}` are escaped.
// Example: `cost: ${x}` becomes `cost: \${x\}`.
export function escapeSnippet(text: string): string {
  return text.replace(/[\\$}]/g, sign => `\\${sign}`);
}

// snippetFor is the snippet over the lines from the first stop's line to the last stop's line:
// their own text, with each stop's text as a numbered placeholder (`${1:todo_field}`, in stop
// order) and `$0` right after the last one, so Tab on the last hole leaves the session. A stop
// whose range runs past its line (the server's whole-line fallback) is the line's text after its
// indent.
// Example: the line `    todo_field: object` with stops at columns 4–14 and 16–22 gives
// `    ${1:todo_field}: ${2:object}$0`.
export function snippetFor(lines: readonly string[], stops: readonly Stop[]): HoleSnippet {
  const startLine = stops[0].range.start.line;
  const endLine = stops[stops.length - 1].range.start.line;
  const spans = stops.map((stop, i) => ({ ...spanOf(lines, stop), n: i + 1 }));
  const text = lines.slice(startLine, endLine + 1)
    .map((line, at) => lineSnippet(line, spans.filter(span => span.line === startLine + at), stops.length))
    .join('\n');
  return { region: { startLine, endLine }, snippet: text };
}

// A stop's stretch of its line, in UTF-16 columns.
interface Span {
  line: number;
  start: number;
  end: number;
}

function spanOf(lines: readonly string[], stop: Stop): Span {
  const { start, end } = stop.range;
  const line = lines[start.line] ?? '';
  if (end.line > start.line) return { line: start.line, start: line.length - line.trimStart().length, end: line.length };
  return { line: start.line, start: Math.min(start.character, line.length), end: Math.min(end.character, line.length) };
}

// lineSnippet is one line with its stops as placeholders (numbered n), the rest escaped; the
// last of `count` stops is followed by `$0`.
function lineSnippet(line: string, spans: readonly (Span & { n: number })[], count: number): string {
  let out = '';
  let at = 0;
  for (const span of spans) {
    out += escapeSnippet(line.slice(at, span.start));
    out += `\${${span.n}:${escapeSnippet(line.slice(span.start, span.end))}}`;
    if (span.n === count) out += '$0';
    at = span.end;
  }
  return out + escapeSnippet(line.slice(at));
}

// messageText is the status line of hole i of n (from 1): what it wants and how to go on. The
// last hole says `Tab done`; the conjure part shows when the Conjure extension is here.
// Example: `hole 1 of 3 · field: its first input · type to replace · Tab next · ctrl+m c conjure`.
export function messageText(i: number, n: number, stop: Stop, conjureActive: boolean): string {
  const parts = [`hole ${i} of ${n}`, `${stop.name}: ${stop.instruction}`, 'type to replace', i === n ? 'Tab done' : 'Tab next'];
  if (conjureActive) parts.push('ctrl+m c conjure');
  return parts.join(' · ');
}

// doneText is the line said when the session ends: done, and where the other files' holes are.
// Example: { cancel_table__test.py: 1 } gives
// `spellbook: holes done · 1 more in cancel_table__test.py (Problems)`.
export function doneText(others: ReadonlyMap<string, number>): string {
  const count = [...others.values()].reduce((sum, n) => sum + n, 0);
  if (count === 0) return 'spellbook: holes done';
  return `spellbook: holes done · ${count} more in ${[...others.keys()].sort().join(', ')} (Problems)`;
}

// A stretch of a document, from start to end.
export interface Range {
  start: Position;
  end: Position;
}

// One edit, shaped as VS Code's TextDocumentContentChangeEvent: the stretch replaced and the
// text put there.
export interface Change {
  range: Range;
  text: string;
}

// shifted is where the ranges are after the edits, applied in order (as VS Code reports them).
// An edit before a range moves it; typing inside a range, or right at its start or end, grows
// it, so a hole keeps covering what you type into it; an edit after it leaves it alone.
// Example: the range 1:4–1:14 (`todo_field`) replaced whole by `id` becomes 1:4–1:6, and an `x`
// typed then at 1:6 makes it 1:4–1:7.
export function shifted(ranges: readonly Range[], changes: readonly Change[]): Range[] {
  return changes.reduce((now, change) => now.map(range => ({
    start: startAfter(range.start, change),
    end: endAfter(range.end, change),
  })), [...ranges]);
}

// stopAt is the index of the range that holds the position (both ends included, so the cursor
// right after typed text is still in its hole), or -1 when none does.
export function stopAt(ranges: readonly Range[], position: Position): number {
  return ranges.findIndex(range => compare(range.start, position) <= 0 && compare(position, range.end) <= 0);
}

// A range's start after an edit: before the edit it stays; at an insertion point it stays (the
// range grows); after the edit it moves; inside the replaced stretch it goes to the edit's start.
function startAfter(p: Position, change: Change): Position {
  const { start, end } = change.range;
  if (compare(p, start) < 0) return p;
  if (compare(p, start) === 0 && compare(start, end) === 0) return p;
  if (compare(p, end) >= 0) return moved(p, change);
  return start;
}

// A range's end after an edit: before the edit it stays; at or after the edit's end it moves
// (an insertion at the end grows the range); inside the replaced stretch it goes to the end of
// the new text.
function endAfter(p: Position, change: Change): Position {
  const { start, end } = change.range;
  if (compare(p, start) < 0) return p;
  if (compare(p, end) >= 0) return moved(p, change);
  return insertedEnd(change);
}

// moved is a position at or after the edit's end, carried by the edit.
function moved(p: Position, change: Change): Position {
  const { end } = change.range;
  const inserted = insertedEnd(change);
  if (p.line === end.line) return { line: inserted.line, character: inserted.character + p.character - end.character };
  return { line: p.line + inserted.line - end.line, character: p.character };
}

// insertedEnd is where the edit's new text ends.
function insertedEnd(change: Change): Position {
  const lines = change.text.split(/\r\n|\r|\n/);
  const { start } = change.range;
  if (lines.length === 1) return { line: start.line, character: start.character + change.text.length };
  return { line: start.line + lines.length - 1, character: lines[lines.length - 1].length };
}

function compare(a: Position, b: Position): number {
  return a.line !== b.line ? a.line - b.line : a.character - b.character;
}

// `1 file`, `2 files`
function counted(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}
