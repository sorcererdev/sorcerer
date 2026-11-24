// The plan preview's words and where its placeholders are (no editor code, so node's test
// runner covers it); planPreview.ts shows them.

// One file of a plan: its path under the root (with `/`), whether it is written, its text.
export interface PlannedFile {
  path: string;
  status: 'new' | 'skipped' | string;
  text: string;
  lines: number;
}

// One place a hole appears: its file, its line (from 1) and the code written there.
export interface HolePlace {
  file: string;
  line: number;
  text: string;
}

// A hole of a plan: a placeholder someone fills, or a value the inputs derived.
export interface Hole {
  name: string;
  kind: string;
  instruction: string;
  source: 'placeholder' | 'derived' | string;
  places: HolePlace[];
}

// A plan, as `spellbook.plan` sends it.
export interface Plan {
  id: string;
  generator: string;
  inputs?: Record<string, string>;
  files: PlannedFile[];
  holes: Hole[];
}

// One file item of the preview's pick: the label shown, a description, the file's path.
export interface FileItem {
  label: string;
  description: string;
  path: string;
}

// A stretch of one line, columns in UTF-16 code units (JavaScript string indices).
export interface LineRange {
  line: number;
  start: number;
  end: number;
}

// planTitle is the preview's title: the generator, how many files it writes and how many holes
// are left to fill (the derived ones are filled already).
// Example: `spellbook: command · 2 files · 3 holes to fill`.
export function planTitle(plan: Plan): string {
  const files = newFiles(plan).length;
  const holes = plan.holes.filter(hole => hole.source === 'placeholder').length;
  return `spellbook: ${plan.generator} · ${counted(files, 'file')} · ${counted(holes, 'hole')} to fill`;
}

// fileItems is one pick item per planned file: `+ path` with its line count for a file written,
// `= path` for one that exists and is skipped.
// Example: `+ src/bistro/booking/commands/cancel_table.py` · `31 lines`.
export function fileItems(plan: Plan): FileItem[] {
  return plan.files.map(file => file.status === 'new'
    ? { label: `+ ${file.path}`, description: counted(file.lines, 'line'), path: file.path }
    : { label: `= ${file.path}`, description: 'exists, skipped', path: file.path });
}

// newFiles is the files the plan writes, in its order.
export function newFiles(plan: Plan): PlannedFile[] {
  return plan.files.filter(file => file.status === 'new');
}

// planUriPath is the path of a planned file's preview document under the `spellbook-plan`
// scheme: the plan id, then the file's path. It ends in the file's own name, so VS Code colours
// the preview as that file's language.
// Example: plan 1a2b3c4d, `src/a.py` → `/1a2b3c4d/src/a.py`.
export function planUriPath(planId: string, filePath: string): string {
  // a URI path, not a file path: `/` is URI syntax here on every platform
  return `/${planId}/${filePath}`;
}

// placeholderRanges is where the placeholders of one planned file are: for each place of a
// placeholder hole in that file, the place's text on its row (found by `lineOf(row)`, rows from
// 0), or the row's text after its indent when the text is empty or not on the row (as the Neovim
// preview lights it). Derived holes are filled already and are not listed.
// Example: `    field: type` on line 10 with the placeholder `field: type` gives
// { line: 9, start: 4, end: 15 }.
export function placeholderRanges(plan: Plan, filePath: string, lineOf: (row: number) => string): LineRange[] {
  return plan.holes
    .filter(hole => hole.source === 'placeholder')
    .flatMap(hole => hole.places.filter(place => place.file === filePath))
    .map(place => rangeOnRow(place.line - 1, place.text, lineOf(place.line - 1)));
}

function rangeOnRow(line: number, text: string, row: string): LineRange {
  const at = text === '' ? -1 : row.indexOf(text);
  if (at >= 0) return { line, start: at, end: at + text.length };
  const indent = row.length - row.trimStart().length;
  return { line, start: indent, end: row.length };
}

// `1 file`, `2 files`
function counted(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}
