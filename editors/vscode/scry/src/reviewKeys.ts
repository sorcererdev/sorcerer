import * as vscode from 'vscode';
import { ExecuteCommandRequest, type LanguageClient } from 'vscode-languageclient/node';
import { LIST, REVIEW } from './keys';

// registerReviewKeys makes the two scry keys that are no code action:
// - review this file now (`ctrl+m s r`): a file with unsaved changes is saved, which makes scry
//   review it; a saved one is sent to the server's `scry.review`, which reviews what changed in
//   it since its last review, at once;
// - list findings (`ctrl+m s l`): the Problems view, where scry's findings are listed with every
//   other diagnostic (scry shows findings in open files only).
export function registerReviewKeys(context: vscode.ExtensionContext, clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(REVIEW.command, () => reviewNow(clientFor)),
    vscode.commands.registerCommand(LIST.command, () => vscode.commands.executeCommand('workbench.actions.view.problems')),
  );
}

async function reviewNow(clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined): Promise<void> {
  const document = vscode.window.activeTextEditor?.document;
  if (!document) return;
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  const client = folder && clientFor(folder);
  if (!client) {
    vscode.window.setStatusBarMessage('scry is not running on this file', 4000);
    return;
  }
  if (document.isDirty) {
    await document.save();
    return;
  }
  const uri = client.code2ProtocolConverter.asUri(document.uri);
  await client.sendRequest(ExecuteCommandRequest.type, { command: 'scry.review', arguments: [uri] })
    .catch((err: unknown) => vscode.window.showWarningMessage(`scry: ${err instanceof Error ? err.message : String(err)}`));
}
