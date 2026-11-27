// The plan preview's words and where its placeholders are (no editor code, so node's test
// runner covers it); planPreview.ts shows them.
import type { Values } from './inputs';

// One hunk of a changed file: the lines it covers on disk (from 1, both included) and its rows
// (` kept`, `-removed`, `+added`).
export interface PlanHunk {
  header: string;
  from: number;
  to: number;
  lines: string[];
}

// One file of a plan: its path under the root (with `/`), whether it is written (`new`), edited
// (`changed`, with what it adds and removes) or left alone, its text after the plan. A file whose
// owned region the plan rewrites names the `region`, and `set_up` counts the items of a region a
// sync sets up.
export interface PlannedFile {
  path: string;
  status: 'new' | 'changed' | 'skipped' | string;
  text: string;
  lines: number;
  added?: number;
  removed?: number;
  hunks?: PlanHunk[];
  region?: string;
  set_up?: number;
}

// The finding a fix plan answers, as `spellbook.plan` echoes it.
export interface PlanFix {
  rule: string;
  file: string;
  line: number;
  fingerprint: string;
  index: number;
  label: string;
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
  fix?: PlanFix;
  // one-line hints: a region not set up, a `regions.toml` that does not load
  notes?: string[];
}

// The generator name of a sync of the owned regions (Spellbook: Sync, a region's quick fix).
export const SYNC = 'sync';

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

// planTitle is the preview's title: the generator, how many files it writes, how many holes
// are left to fill (the derived ones are filled already) and the inputs taken without asking,
// so the person sees what was picked for them. A fix plan names the rule and the fix, counts
// changed files and new ones, and names the inputs the finding settled.
// Example: `spellbook: command · 2 files · 3 holes to fill · app bistro, service booking (the only choices)`.
// Example: `spellbook: fix commands-use-ports · Use a port the service has · 2 files changed · 0 holes to fill`.
// A sync counts the files whose regions it rewrites: `spellbook: sync · 1 file changed`.
export function planTitle(plan: Plan, taken: Values = {}, settled: Values = {}): string {
  if (plan.generator === SYNC) return `spellbook: sync · ${fixCounts(plan)}`;
  const holes = counted(plan.holes.filter(hole => hole.source === 'placeholder').length, 'hole');
  if (plan.fix) return `spellbook: fix ${plan.fix.rule} · ${plan.fix.label} · ${fixCounts(plan)} · ${holes} to fill${takenText(taken)}${settledText(settled)}`;
  return `spellbook: ${plan.generator} · ${counted(newFiles(plan).length, 'file')} · ${holes} to fill${takenText(taken)}`;
}

// `2 files changed`, then ` · 4 new` when the plan writes new files too.
function fixCounts(plan: Plan): string {
  const changed = `${counted(changedFiles(plan).length, 'file')} changed`;
  const fresh = newFiles(plan).length;
  return fresh === 0 ? changed : `${changed} · ${fresh} new`;
}

// ` · app bistro (the only choice)`, or nothing when every input was asked or given.
function takenText(taken: Values): string {
  const names = Object.entries(taken).map(([name, value]) => `${name} ${value}`);
  if (names.length === 0) return '';
  return ` · ${names.join(', ')} (the only ${names.length === 1 ? 'choice' : 'choices'})`;
}

// ` · app bistro, unit close_table (from the finding)`, or nothing.
function settledText(settled: Values): string {
  const names = Object.entries(settled).map(([name, value]) => `${name} ${value}`);
  return names.length === 0 ? '' : ` · ${names.join(', ')} (from the finding)`;
}

// fileItems is one pick item per planned file: `+ path` with its line count for a file written,
// `~ path` with `+a −r` for a changed one (`region NAME: +a −r` when what changes is an owned
// region, `region NAME: set up, N items` when a sync sets it up), `= path` for one that exists
// and is skipped.
// Example: `+ src/bistro/booking/commands/cancel_table.py` · `31 lines`; `~ close_table.py` · `+3 −1`;
// `~ src/booking/commands/mod.rs` · `region modules: +1 −0`.
export function fileItems(plan: Plan): FileItem[] {
  return plan.files.map(file => {
    if (file.status === 'new') return { label: `+ ${file.path}`, description: counted(file.lines, 'line'), path: file.path };
    if (file.status === 'changed') return { label: `~ ${file.path}`, description: changedText(file), path: file.path };
    return { label: `= ${file.path}`, description: 'exists, skipped', path: file.path };
  });
}

// `+3 −1`, `region modules: +1 −0` or `region modules: set up, 2 items`.
function changedText(file: PlannedFile): string {
  const counts = `+${file.added ?? 0} −${file.removed ?? 0}`;
  if (!file.region) return counts;
  if (file.set_up !== undefined) return `region ${file.region}: set up, ${counted(file.set_up, 'item')}`;
  return `region ${file.region}: ${counts}`;
}

// syncedText is the status line once a sync is applied, or what is said when there was nothing
// to sync: the regions up to date, then the plan's notes (a region not set up).
// Example: `spellbook: synced 1 file`; `spellbook: regions up to date`.
export function syncedText(changed: number, notes: string[] = []): string {
  const said = changed === 0 ? 'spellbook: regions up to date' : `spellbook: synced ${counted(changed, 'file')}`;
  return [said, ...notes].join(' · ');
}

// newFiles is the files the plan writes, in its order.
export function newFiles(plan: Plan): PlannedFile[] {
  return plan.files.filter(file => file.status === 'new');
}

// changedFiles is the existing files the plan edits, in its order.
export function changedFiles(plan: Plan): PlannedFile[] {
  return plan.files.filter(file => file.status === 'changed');
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
