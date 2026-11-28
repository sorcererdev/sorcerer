import { execFile } from 'node:child_process';
import * as vscode from 'vscode';
import { folderHere } from './asker';
import { holeLabel, openHoles } from './openHoles';

// registerHoles makes List holes (`spellbook.holes`, `ctrl+m b h`): the holes still open in the
// project, from `spellbook holes --json`, in a picker; the one picked opens at its line.
// `notFoundText` is what to say when no spellbook binary was found (the install line).
export function registerHoles(context: vscode.ExtensionContext, binaryFound: () => string | undefined, notFoundText: () => string): void {
  context.subscriptions.push(vscode.commands.registerCommand('spellbook.holes', () => listHoles(binaryFound(), notFoundText)));
}

async function listHoles(binary: string | undefined, notFoundText: () => string): Promise<void> {
  if (!binary) {
    void vscode.window.showWarningMessage(notFoundText());
    return;
  }
  const folder = await folderHere();
  if (!folder) return;
  const stdout = await new Promise<string | Error>(resolve =>
    execFile(binary, ['holes', '--json'], { cwd: folder.uri.fsPath, maxBuffer: 16 * 1024 * 1024 }, (err, out, stderr) =>
      resolve(err ? new Error(stderr.trim() || err.message) : out)));
  if (stdout instanceof Error) {
    void vscode.window.showErrorMessage(`spellbook: holes could not run: ${stdout.message}`);
    return;
  }
  const holes = openHoles(stdout);
  if (typeof holes === 'string') {
    void vscode.window.showErrorMessage(holes);
    return;
  }
  if (holes.length === 0) {
    vscode.window.setStatusBarMessage('spellbook: no open holes', 6000);
    return;
  }
  const picked = await vscode.window.showQuickPick(holes.map(hole => ({ label: holeLabel(hole), hole })), { placeHolder: 'Which hole?' });
  if (!picked) return;
  const uri = vscode.Uri.joinPath(folder.uri, ...picked.hole.file.split('/'));
  const at = new vscode.Position(Math.max(0, picked.hole.line - 1), 0);
  await vscode.window.showTextDocument(uri, { selection: new vscode.Range(at, at) });
}
