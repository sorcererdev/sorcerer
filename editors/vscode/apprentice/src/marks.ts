// Which lines carry a finished task's mark (no editor code, so node's test runner covers it);
// taskMarks.ts draws the gutter icon on them.

// The part of a diagnostic this file reads.
export interface MarkDiagnostic {
  source?: string;
  code?: unknown;
  line: number;
}

// taskLines is the lines (from 0) of the diagnostics that are a finished task's mark, `✦
// apprentice: …` on the first changed line of each file: source `apprentice`, code `task`. A
// region's mark (code `region`), the agent's own (`said`), a question's (`ask`) and another
// tool's diagnostic are not. Each line is once, in ascending order.
export function taskLines(diagnostics: readonly MarkDiagnostic[]): number[] {
  const lines = diagnostics.filter(d => d.source === 'apprentice' && d.code === 'task').map(d => d.line);
  return [...new Set(lines)].sort((a, b) => a - b);
}
