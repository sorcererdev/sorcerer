// List holes' pure part (no editor code, so node's test runner covers it): the reading of
// `spellbook holes --json`. holesCommand.ts runs it.

// One hole still open: its name, the file (from the project root, with `/`) and line (from 1)
// it is at, and what it wants.
export interface OpenHole {
  name: string;
  file: string;
  line: number;
  instruction: string;
}

// openHoles reads what `spellbook holes --json` printed: a list of holes, or the words to say
// when it is not one.
//
// Example: `[{"name":"field","file":"src/a.py","line":3,"kind":"expr","instruction":"its first input"}]`
// is one hole, `field` at src/a.py:3.
export function openHoles(stdout: string): OpenHole[] | string {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    return 'spellbook: holes printed something unreadable';
  }
  if (!Array.isArray(parsed)) return 'spellbook: holes printed something unreadable';
  return parsed
    .filter((h): h is OpenHole => typeof h?.name === 'string' && typeof h?.file === 'string' && typeof h?.line === 'number')
    .map(h => ({ name: h.name, file: h.file, line: h.line, instruction: typeof h.instruction === 'string' ? h.instruction : '' }));
}

// holeLabel is a hole as the picker lists it: `field · src/a.py:3 · its first input`.
export function holeLabel(hole: OpenHole): string {
  return [hole.name, `${hole.file}:${hole.line}`, hole.instruction].filter(part => part !== '').join(' · ');
}
