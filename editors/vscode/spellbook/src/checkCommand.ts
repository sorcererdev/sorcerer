import { execFile } from 'node:child_process';
import { relative, sep } from 'node:path';
import * as vscode from 'vscode';
import { folderHere } from './asker';
import { checkFailedText, type FindingEntry, findingEntries, parseReport } from './check';

// What `spellbook check` printed and how it ended.
interface CheckRun {
  exit: number;
  stdout: string;
  stderr: string;
}

// registerCheck makes spellbook: Check the project's rules (`spellbook.check`, `ctrl+m b k`): the project's rules run
// over the whole folder into Problems, under the collection `spellbook check`. A file opened
// later leaves the collection, since the server shows its findings from then on.
// `notFoundText` is what to say when no spellbook binary was found (the install line).
export function registerCheck(context: vscode.ExtensionContext, binaryFound: () => string | undefined, notFoundText: () => string): void {
  const collection = vscode.languages.createDiagnosticCollection('spellbook check');
  context.subscriptions.push(
    collection,
    vscode.commands.registerCommand('spellbook.check', () => checkHere(collection, binaryFound(), notFoundText)),
    vscode.workspace.onDidOpenTextDocument(doc => collection.delete(doc.uri)),
  );
}

// checkHere runs the rules in the folder here:
// 1. `spellbook check --json` in the folder's root (exit 0: they hold, 1: an error finding,
//    anything else: it could not run, which is said);
// 2. its findings, open files left out, replace the collection;
// 3. Problems opens when there are any, else the status bar says the conventions hold.
async function checkHere(collection: vscode.DiagnosticCollection, binary: string | undefined, notFoundText: () => string): Promise<void> {
  if (!binary) {
    void vscode.window.showWarningMessage(notFoundText());
    return;
  }
  const folder = await folderHere();
  if (!folder) return;
  const run = await checked(binary, folder.uri.fsPath);
  if (run.exit !== 0 && run.exit !== 1) {
    void vscode.window.showErrorMessage(checkFailedText(run.stderr, run.stdout));
    return;
  }
  const findings = parseReport(run.stdout);
  if (typeof findings === 'string') {
    void vscode.window.showErrorMessage(findings);
    return;
  }
  const entries = findingEntries(findings, openFilesUnder(folder));
  collection.clear();
  for (const [uri, diagnostics] of diagnosticsByFile(folder, entries)) collection.set(uri, diagnostics);
  if (entries.length > 0) await vscode.commands.executeCommand('workbench.actions.view.problems');
  else vscode.window.setStatusBarMessage('spellbook: conventions hold', 10_000);
}

// checked runs the program with an argument list (never a shell) and gives how it ended.
function checked(binary: string, cwd: string): Promise<CheckRun> {
  return new Promise(resolve => {
    execFile(binary, ['check', '--json'], { cwd, maxBuffer: 16 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (!err) return resolve({ exit: 0, stdout, stderr });
      // a program that ran and exited non-zero has its exit code; one that did not start has none
      if (typeof err.code === 'number') return resolve({ exit: err.code, stdout, stderr });
      resolve({ exit: -1, stdout, stderr: `${stderr}${err.message}` });
    });
  });
}

// openFilesUnder is the open files of the folder, root-relative with `/`.
function openFilesUnder(folder: vscode.WorkspaceFolder): Set<string> {
  return new Set(vscode.workspace.textDocuments
    .filter(doc => doc.uri.scheme === 'file' && vscode.workspace.getWorkspaceFolder(doc.uri)?.uri.toString() === folder.uri.toString())
    .map(doc => relative(folder.uri.fsPath, doc.uri.fsPath).split(sep).join('/')));
}

// diagnosticsByFile is the entries as VS Code diagnostics, each on its whole line, by file.
function diagnosticsByFile(folder: vscode.WorkspaceFolder, entries: FindingEntry[]): Map<vscode.Uri, vscode.Diagnostic[]> {
  const byFile = new Map<string, { uri: vscode.Uri; diagnostics: vscode.Diagnostic[] }>();
  for (const entry of entries) {
    const uri = vscode.Uri.joinPath(folder.uri, ...entry.file.split('/'));
    const slot = byFile.get(uri.toString()) ?? { uri, diagnostics: [] };
    slot.diagnostics.push(diagnosticOf(entry));
    byFile.set(uri.toString(), slot);
  }
  return new Map([...byFile.values()].map(slot => [slot.uri, slot.diagnostics]));
}

function diagnosticOf(entry: FindingEntry): vscode.Diagnostic {
  // a file not open: the line's length is not known, and VS Code clamps the end to the line
  const range = new vscode.Range(entry.line, 0, entry.line, Number.MAX_SAFE_INTEGER);
  const diagnostic = new vscode.Diagnostic(range, entry.message, severities[entry.severity]);
  diagnostic.source = 'spellbook';
  diagnostic.code = entry.code;
  return diagnostic;
}

const severities: Record<FindingEntry['severity'], vscode.DiagnosticSeverity> = {
  error: vscode.DiagnosticSeverity.Error,
  warning: vscode.DiagnosticSeverity.Warning,
  hint: vscode.DiagnosticSeverity.Hint,
};
