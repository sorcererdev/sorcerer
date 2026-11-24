// The words said after Spellbook: New (no editor code, so node's test runner covers it).

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

// `1 file`, `2 files`
function counted(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}
