// Which stretches carry a hole's mark (no editor code, so node's test runner covers it);
// holeMarks.ts draws the hole look on them.

// A position in a document: a line from 0 and a UTF-16 column.
export interface MarkPosition {
  line: number;
  character: number;
}

// The part of a diagnostic this file reads.
export interface MarkDiagnostic {
  source?: string;
  code?: unknown;
  range: { start: MarkPosition; end: MarkPosition };
}

// holeRanges is the ranges of the diagnostics that are a hole's mark (source `spellbook`, code
// `hole`; a rule finding is not one, and neither is another tool's diagnostic), each once, in
// document order. A mark's range is its placeholder.
// Example: a hole mark on 9:4–9:14 and a commands-use-ports finding give [9:4–9:14].
export function holeRanges(diagnostics: readonly MarkDiagnostic[]): MarkDiagnostic['range'][] {
  const holes = diagnostics.filter(d => d.source === 'spellbook' && d.code === 'hole').map(d => d.range);
  const unique = [...new Map(holes.map(range => [key(range), range])).values()];
  return unique.sort((a, b) => a.start.line - b.start.line || a.start.character - b.start.character);
}

function key(range: MarkDiagnostic['range']): string {
  return `${range.start.line}:${range.start.character}-${range.end.line}:${range.end.character}`;
}
