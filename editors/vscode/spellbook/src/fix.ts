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

// preferredFix is the action `ctrl+m f` runs: the first action that runs `spellbook.fix` (the
// server lists the preferred one first).
export function preferredFix<T extends FixAction>(actions: readonly T[]): T | undefined {
  const fixes = actions.filter(action => action.command?.command === 'spellbook.fix');
  return fixes.find(action => action.isPreferred) ?? fixes[0];
}

// fixedText is the status line once a fix is applied: the rule and file, the files changed, and
// the files written and holes left when there are any.
// Example: `spellbook: fixed commands-use-ports in close_table.py · changed 2 files · wrote 4 files · 2 holes, marked`.
export function fixedText(rule: string, file: string, changed: number, written: number, holes: number): string {
  const parts = [`spellbook: fixed ${rule} in ${file.split('/').at(-1) ?? file}`, `changed ${counted(changed, 'file')}`];
  if (written > 0) parts.push(`wrote ${counted(written, 'file')}`);
  if (holes > 0) parts.push(`${counted(holes, 'hole')}, marked`);
  return parts.join(' · ');
}

function counted(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}
