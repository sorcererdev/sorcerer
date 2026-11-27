// What a server says it is working on, from the `sorcerer/working` notification: the units
// whose first line shows a grey `⟳ scry: reviewing…` after it. This file holds no editor code, so
// node's test runner covers it; inline.ts draws what it returns.

// One unit being worked on: the document's uri, the unit's first line (from 0) and the text to
// write after it.
export interface WorkingUnit {
  uri: string;
  line: number;
  text: string;
}

// parseWorking reads the notification's params, `{ units: [{ uri, line, text }] }`, and keeps
// the units that are well formed; anything else is no units (nothing is drawn).
export function parseWorking(params: unknown): WorkingUnit[] {
  const units = (params as { units?: unknown } | null | undefined)?.units;
  if (!Array.isArray(units)) return [];
  return units.filter(isUnit).map(({ uri, line, text }) => ({ uri, line, text }));
}

function isUnit(value: unknown): value is WorkingUnit {
  const unit = value as Partial<WorkingUnit> | null | undefined;
  return typeof unit?.uri === 'string' && typeof unit.text === 'string'
    && typeof unit.line === 'number' && Number.isInteger(unit.line) && unit.line >= 0;
}

// forDocument is the units in one document, in the order the server sent them.
export function forDocument(units: WorkingUnit[], uri: string): WorkingUnit[] {
  return units.filter(unit => unit.uri === uri);
}

// spelledAs is the units with each uri passed through `spell`. The server sends
// `file:///C:/a.py` and VS Code names that document `file:///c%3A/a.py`, so extension.ts spells
// each uri as VS Code does (`uri => vscode.Uri.parse(uri).toString()`) before forDocument matches.
export function spelledAs(units: WorkingUnit[], spell: (uri: string) => string): WorkingUnit[] {
  return units.map(unit => ({ ...unit, uri: spell(unit.uri) }));
}

// statusText is the status bar's text while units are worked on: the spinning icon and the
// first unit's text without its own `⟳`, and how many more there are; undefined when none is.
//
// Example: two units `⟳ lackey: researching…` and `⟳ lackey: working…` give
// `$(sync~spin) lackey: researching… (+1)`.
export function statusText(units: readonly WorkingUnit[]): string | undefined {
  const first = units[0];
  if (!first) return undefined;
  const more = units.length > 1 ? ` (+${units.length - 1})` : '';
  return `$(sync~spin) ${first.text.replace(/^\u27f3\s*/, '')}${more}`;
}
