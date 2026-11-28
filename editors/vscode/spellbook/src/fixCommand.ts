import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { folderHere } from './asker';
import { dirtyText, type FixAction, fixArgument, fixedText, fixThis, notShown, preferredFix } from './fix';
import { finish, flowIn, type AskerFor } from './newCommand';

// registerFix makes the three commands of a finding's fix: `spellbook.fix` (the command of the
// server's code action: the lightbulb and `ctrl+.` run it), `spellbook.fixHere` (`ctrl+m b f`,
// which runs the line's spellbook fix without the menu) and `spellbook.fixThis` (`ctrl+m f`, fix
// this: the spellbook fix, else the line's preferred quick fix of any server).
export function registerFix(context: vscode.ExtensionContext, clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined,
  askerFor: AskerFor): void {
  context.subscriptions.push(
    vscode.commands.registerCommand('spellbook.fix', (argument?: unknown) => fixHere(clientFor, askerFor, argument)),
    vscode.commands.registerCommand('spellbook.fixHere', fixOnLine),
    vscode.commands.registerCommand('spellbook.fixThis', fixThisLine),
  );
}

// fixHere runs the fix the code action carries:
// 1. a buffer with unsaved changes is refused, by name;
// 2. otherwise the New flow runs with the fix's generator, the inputs the finding settled, and
//    the finding with the buffer's version, asking only what is missing;
// 3. the server lands the changes through the editor (one undo step per file, VS Code's own) and
//    the status line says what was fixed; holes are marked, and a hole session starts as after New.
async function fixHere(clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined, askerFor: AskerFor, raw: unknown): Promise<void> {
  const folder = await folderHere();
  const argument = fixArgument(raw);
  if (!folder || !argument) return;
  const asker = askerFor(folder);
  const document = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === vscode.Uri.joinPath(folder.uri, ...argument.fix.file.split('/')).toString());
  if (document?.isDirty) {
    asker.say('warn', dirtyText(argument.fix.file));
    return;
  }
  const { generator, inputs, missing, fix } = argument;
  const version = document?.version ?? 0;
  const reply = await flowIn(clientFor, folder, asker, { generator, inputs, missing, fix: { ...fix, version } });
  if (!reply) return;
  const changed = reply.changed ?? [];
  await finish(folder, reply, asker, (written, holes) =>
    fixedText(fix.rule, fix.file, changed.length, written, holes, notShown(changed, path => inATab(folder, path))));
}

// fixOnLine asks the servers for the code actions on the cursor's line, as the lightbulb does,
// and runs the preferred spellbook fix; with none it says so.
async function fixOnLine(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return;
  const line = editor.document.lineAt(editor.selection.active.line).range;
  const actions = await vscode.commands.executeCommand<(vscode.Command | vscode.CodeAction)[]>(
    'vscode.executeCodeActionProvider', editor.document.uri, line, vscode.CodeActionKind.QuickFix.value);
  const action = preferredFix((actions ?? []).map(asFixAction));
  if (!action?.command) {
    vscode.window.setStatusBarMessage('spellbook: no fix on this line', 4000);
    return;
  }
  await vscode.commands.executeCommand(action.command.command, ...(action.command.arguments ?? []));
}

// fixThisLine is fix this (`ctrl+m f`): the line's quick fixes, as the lightbulb finds them, and
// what fixThis picks among them run, the menu opened, or "nothing to fix" said.
async function fixThisLine(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return;
  const line = editor.document.lineAt(editor.selection.active.line).range;
  const actions = await vscode.commands.executeCommand<(vscode.Command | vscode.CodeAction)[]>(
    'vscode.executeCodeActionProvider', editor.document.uri, line, vscode.CodeActionKind.QuickFix.value);
  const picked = fixThis((actions ?? []).map(item => ({ ...asFixAction(item), item })));
  if ('nothing' in picked) {
    vscode.window.setStatusBarMessage('nothing to fix on this line', 4000);
    return;
  }
  if ('menu' in picked) {
    await vscode.commands.executeCommand('editor.action.quickFix');
    return;
  }
  const action = picked.run.item as vscode.CodeAction;
  if ('kind' in action && action.edit) await vscode.workspace.applyEdit(action.edit);
  if (picked.run.command) await vscode.commands.executeCommand(picked.run.command.command, ...(picked.run.command.arguments ?? []));
}

// a bare command is its own action
function asFixAction(item: vscode.Command | vscode.CodeAction): FixAction {
  const action = item as vscode.CodeAction;
  if (typeof item.command === 'object' || 'kind' in item) return { title: item.title, isPreferred: action.isPreferred, command: action.command };
  return { title: item.title, command: item as vscode.Command };
}

// inATab says whether a tab in any group shows the file (a path from the folder's root).
function inATab(folder: vscode.WorkspaceFolder, path: string): boolean {
  const uri = vscode.Uri.joinPath(folder.uri, ...path.split('/')).toString();
  return vscode.window.tabGroups.all.some(group => group.tabs.some(tab =>
    tab.input instanceof vscode.TabInputText && tab.input.uri.toString() === uri));
}
