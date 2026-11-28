// Spellbook: Fix's pure part (no editor code, so node's test runner covers it): the code
// action's argument, what the plan is asked, and the words said. fixCommand.ts runs it.

// The finding a fix answers, as the server's code action names it. `version` is null in the
// action; the editor fills in the document's version when the fix runs.
export interface FixRef {
  file: string;
  line: number;
  rule: string;
  index: number;
  label: string;
  version: number | null;
}

// The argument of the code action's `spellbook.fix` command: the fix's generator, the inputs the
// finding settled, the inputs still to ask, and the finding.
export interface FixArgument {
  generator: string;
  inputs: Record<string, string>;
  missing: string[];
  fix: FixRef;
}

// A code action as the editor returns it: its title, whether it is the preferred one, and the
// command it runs.
export interface FixAction {
  title: string;
  isPreferred?: boolean;
  command?: { command: string; arguments?: unknown[] };
}

// fixArgument reads the command's first argument, or undefined when it is not the server's shape.
export function fixArgument(raw: unknown): FixArgument | undefined {
  const arg = raw as Partial<FixArgument> | undefined;
  const fix = arg?.fix as Partial<FixRef> | undefined;
  if (!arg || typeof arg.generator !== 'string' || typeof arg.inputs !== 'object' || arg.inputs === null) return undefined;
  if (!Array.isArray(arg.missing) || !fix) return undefined;
  const shaped = typeof fix.file === 'string' && typeof fix.line === 'number' && typeof fix.rule === 'string'
    && typeof fix.index === 'number' && typeof fix.label === 'string';
  return shaped ? arg as FixArgument : undefined;
}

// fixRequest is the `fix` argument `spellbook.plan` takes: the finding and the buffer's version.
// Example: line 4 of close_table.py, rule commands-use-ports, fix 1, version 12.
export function fixRequest(fix: FixRef, version: number): { file: string; line: number; rule: string; index: number; version: number } {
  return { file: fix.file, line: fix.line, rule: fix.rule, index: fix.index, version };
}

// dirtyText is what is said when the buffer has unsaved changes.
// Example: `spellbook: save close_table.py first; a fix is planned against the file on disk`.
export function dirtyText(file: string): string {
  return `spellbook: save ${file.split('/').at(-1) ?? file} first; a fix is planned against the file on disk`;
}

// preferredFix is the action `ctrl+m b f` runs (and `ctrl+m f` first): the first action that runs `spellbook.fix` (the
// server lists the preferred one first).
export function preferredFix<T extends FixAction>(actions: readonly T[]): T | undefined {
  const fixes = actions.filter(action => action.command?.command === 'spellbook.fix');
  return fixes.find(action => action.isPreferred) ?? fixes[0];
}

// FixThis is what `ctrl+m f` (fix this) does on a line: run the spellbook fix, run another
// server's preferred quick fix, open the quick-fix menu (several fixes, none preferred), or say
// there is nothing to fix.
export type FixThis<T> = { run: T } | { menu: true } | { nothing: true };

// fixThis picks what fix this does with the line's quick fixes: the spellbook fix first, else
// the one preferred quick fix of any server, else the menu when there are fixes, else nothing.
// The actions of scry, conjure and lackey are quick fixes by kind but fix nothing, so they are
// left out.
//
// Example: on a line with ruff's preferred `Remove unused import` and scry's `scry: Show why`,
// fix this runs ruff's fix.
export function fixThis<T extends FixAction>(actions: readonly T[]): FixThis<T> {
  const spellbook = preferredFix(actions);
  if (spellbook) return { run: spellbook };
  const fixes = actions.filter(action => !/^(scry|conjure|lackey): /.test(action.title) && action.title !== 'Conjure here');
  const preferred = fixes.filter(action => action.isPreferred);
  if (preferred.length === 1) return { run: preferred[0] };
  if (fixes.length === 1) return { run: fixes[0] };
  return fixes.length > 0 ? { menu: true } : { nothing: true };
}

// fixedText is the status line once a fix is applied: the rule and file, the files changed, the
// changed files no tab shows (the edit leaves them unsaved, out of sight, so they are named), and
// the files written and holes left when there are any. At most three are named.
// Example: `spellbook: fixed commands-use-ports in close_table.py · changed 2 files ·
// close_table__test.py not shown, unsaved · wrote 4 files · 2 holes, marked`.
export function fixedText(rule: string, file: string, changed: number, written: number, holes: number,
  hidden: readonly string[] = []): string {
  const parts = [`spellbook: fixed ${rule} in ${baseName(file)}`, `changed ${counted(changed, 'file')}`];
  if (hidden.length > 0) parts.push(`${namedAtMostThree(hidden)} not shown, unsaved`);
  if (written > 0) parts.push(`wrote ${counted(written, 'file')}`);
  if (holes > 0) parts.push(`${counted(holes, 'hole')}, marked`);
  return parts.join(' · ');
}

// notShown is the changed files (paths from the project's root) that `shown` says no tab holds.
// Example: changed a.py and b.py, a tab on a.py: [b.py].
export function notShown(changed: readonly string[], shown: (path: string) => boolean): string[] {
  return changed.filter(path => !shown(path));
}

// Example: [a.py, b.py] gives `a.py, b.py`; five files give `a.py, b.py, c.py and 2 more`.
function namedAtMostThree(paths: readonly string[]): string {
  const names = paths.slice(0, 3).map(baseName).join(', ');
  return paths.length > 3 ? `${names} and ${paths.length - 3} more` : names;
}

function baseName(path: string): string {
  return path.split('/').at(-1) ?? path;
}

function counted(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}
