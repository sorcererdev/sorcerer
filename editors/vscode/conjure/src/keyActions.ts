import * as vscode from 'vscode';
import { actionTitled, KEYS } from './keys';

// registerKeys makes the four commands the `ctrl+m` chords call (see KEYS and package.json).
export function registerKeys(context: vscode.ExtensionContext): void {
  for (const { command, title, none } of KEYS) {
    context.subscriptions.push(vscode.commands.registerCommand(command, () => runActionTitled(title, none)));
  }
}

// runActionTitled asks the servers for the code actions at the cursor (the selection, when
// there is one), as the lightbulb does, and runs the one with this title; with none it says
// `none` in the status bar.
async function runActionTitled(title: string, none: string): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return;
  const actions = await vscode.commands.executeCommand<(vscode.Command | vscode.CodeAction)[]>(
    'vscode.executeCodeActionProvider', editor.document.uri, editor.selection);
  const action = actionTitled(actions ?? [], title);
  if (!action) {
    vscode.window.setStatusBarMessage(none, 4000);
    return;
  }
  // the server's actions are code actions that hold a command; a bare command is its own
  const command: vscode.Command | undefined = 'kind' in action || typeof action.command === 'object'
    ? (action as vscode.CodeAction).command : (action as vscode.Command);
  if (command) await vscode.commands.executeCommand(command.command, ...(command.arguments ?? []));
}
