import * as vscode from 'vscode';
import { actionsFor, type Key, KEYS } from './keys';

type ServerAction = vscode.Command | vscode.CodeAction;

// registerKeys makes the seven commands the `ctrl+m` chords call (see KEYS and package.json).
export function registerKeys(context: vscode.ExtensionContext): void {
  for (const key of KEYS) {
    context.subscriptions.push(vscode.commands.registerCommand(key.command, () => runKey(key)));
  }
}

// runKey asks the servers for the code actions at the cursor (the selection, when there is
// one), as the lightbulb does, and runs the key's action; a key that chooses lists the matching
// actions to pick from; with none it says the key's `none` in the status bar.
async function runKey(key: Key): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return;
  const actions = await vscode.commands.executeCommand<ServerAction[]>(
    'vscode.executeCodeActionProvider', editor.document.uri, editor.selection);
  const matching = actionsFor(actions ?? [], key);
  const action = key.choose && matching.length > 1 ? await picked(matching) : matching[0];
  if (matching.length === 0) vscode.window.setStatusBarMessage(key.none, 4000);
  if (action) await run(action);
}

async function picked(actions: ServerAction[]): Promise<ServerAction | undefined> {
  const items = actions.map(action => ({ label: action.title, action }));
  return (await vscode.window.showQuickPick(items, { placeHolder: 'What was this line?' }))?.action;
}

// the server's actions are code actions that hold a command; a bare command is its own
async function run(action: ServerAction): Promise<void> {
  const command: vscode.Command | undefined = 'kind' in action || typeof action.command === 'object'
    ? (action as vscode.CodeAction).command : (action as vscode.Command);
  if (command) await vscode.commands.executeCommand(command.command, ...(command.arguments ?? []));
}
