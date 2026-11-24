// Which lines carry a conjured change's mark (no editor code, so node's test runner covers it);
// conjuredMarks.ts draws the gutter icon on them.

// The part of a diagnostic this file reads.
export interface MarkDiagnostic {
  source?: string;
  code?: unknown;
  line: number;
}

// conjuredLines is the lines (from 0) of the diagnostics that are a conjured change's mark:
// source `conjure`, code `change`. The "no change" mark (code `no-change`) is not one, and
// neither is another tool's diagnostic. Each line is once, in ascending order.
export function conjuredLines(diagnostics: readonly MarkDiagnostic[]): number[] {
  const lines = diagnostics.filter(d => d.source === 'conjure' && d.code === 'change').map(d => d.line);
  return [...new Set(lines)].sort((a, b) => a - b);
}
