import * as vscode from 'vscode';
import type { Asker } from './newFlow';
import type { PlanPreviews } from './planPreview';

// vscodeAsker is the Asker of VS Code's own pickers: a QuickPick for the generator and for an
// enum's choices, an InputBox for typed inputs, the plan preview for the plan, and messages for
// what is said. Esc anywhere answers undefined, which ends the flow silently.
export function vscodeAsker(previews: PlanPreviews): Asker {
  return {
    pickGenerator: async generators => {
      const items = generators.map(generator => ({ label: generator.name, description: generator.summary, generator }));
      const picked = await vscode.window.showQuickPick(items, { title: 'spellbook: new', placeHolder: 'Generator' });
      return picked?.generator;
    },
    pickChoice: (input, items) => Promise.resolve(vscode.window.showQuickPick(items, { title: 'spellbook: new', placeHolder: input.prompt })),
    askText: (input, prompt) => Promise.resolve(vscode.window.showInputBox({ title: 'spellbook: new', prompt, placeHolder: input.name })),
    showPlan: plan => previews.showPlan(plan),
    say: (level, text) => {
      if (level === 'error') void vscode.window.showErrorMessage(text);
      else if (level === 'warn') void vscode.window.showWarningMessage(text);
      else void vscode.window.showInformationMessage(text);
    },
  };
}

// folderHere is the workspace folder a command works in: the active editor's, else the only
// one, else the one the person picks (undefined when none is open or the pick is dismissed).
export async function folderHere(): Promise<vscode.WorkspaceFolder | undefined> {
  const editor = vscode.window.activeTextEditor;
  const mine = editor && vscode.workspace.getWorkspaceFolder(editor.document.uri);
  if (mine) return mine;
  const folders = vscode.workspace.workspaceFolders ?? [];
  if (folders.length <= 1) return folders[0];
  return vscode.window.showWorkspaceFolderPick({ placeHolder: 'spellbook: which folder?' });
}
