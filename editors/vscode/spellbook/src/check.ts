// Spellbook: Check's reading of `spellbook check --json` (no editor code, so node's test runner
// covers it); checkCommand.ts runs the program and fills Problems.

// One finding of `spellbook check --json`: a rule broken on a line (from 1) of a file under the
// root (with `/`).
export interface Finding {
  rule: string;
  file: string;
  line: number;
  severity: 'error' | 'warn' | 'hint' | string;
  message: string;
}

// One Problems entry to make: the line from 0 and VS Code's word for the severity.
export interface FindingEntry {
  file: string;
  line: number;
  severity: 'error' | 'warning' | 'hint';
  code: string;
  message: string;
}

// parseReport is the findings of the check's output, or the sentence to say when the output is
// not the report.
// Example: `{"checked": "…", "findings": []}` gives []; `oops` gives
// `spellbook: check printed something unreadable`.
export function parseReport(stdout: string): Finding[] | string {
  try {
    const report = JSON.parse(stdout) as { findings?: unknown };
    if (typeof report === 'object' && report !== null && !Array.isArray(report) && (report.findings === undefined || Array.isArray(report.findings))) {
      return (report.findings ?? []) as Finding[];
    }
  } catch {
    // said below
  }
  return 'spellbook: check printed something unreadable';
}

// findingEntries is the Problems entries of the findings, leaving out the files in `openFiles`
// (root-relative, with `/`): the server already shows the findings of an open file, and a
// finding listed twice reads as two.
// Example: { rule: 'commands-use-ports', file: 'src/a.py', line: 5, severity: 'error' } gives an
// error on line 4 (from 0) of src/a.py, code commands-use-ports.
export function findingEntries(findings: readonly Finding[], openFiles: ReadonlySet<string>): FindingEntry[] {
  return findings
    .filter(finding => !openFiles.has(finding.file))
    .map(finding => ({
      file: finding.file,
      line: Math.max(0, finding.line - 1),
      severity: severityOf(finding.severity),
      code: finding.rule,
      message: finding.message,
    }));
}

function severityOf(severity: string): FindingEntry['severity'] {
  if (severity === 'error') return 'error';
  if (severity === 'warn') return 'warning';
  return 'hint';
}

// checkFailedText is what is said when the check could not run (exit 2): its error output, or
// its standard output when that is empty.
// Example: `spellbook: check could not run: spellbook is off in this project`.
export function checkFailedText(stderr: string, stdout: string): string {
  const why = (stderr.trim() !== '' ? stderr : stdout).trim();
  return `spellbook: check could not run: ${why}`;
}
