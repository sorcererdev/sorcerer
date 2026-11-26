import * as vscode from 'vscode';
import type { Values } from './inputs';
import { changedFiles, type FileItem, fileItems, type LineRange, newFiles, type Plan, placeholderRanges, planTitle, planUriPath } from './plan';

// The scheme of the plan's preview documents: virtual, so they are read-only, and each path
// ends in the planned file's name, so VS Code colours it as that file's language.
export const PLAN_SCHEME = 'spellbook-plan';
// The scheme of a changed file's text on disk, the left side of its diff.
export const BEFORE_SCHEME = 'spellbook-plan-before';

// Draws the placeholder look on a preview's ranges (the hole look, holeMarks.ts).
export type PlaceholderLook = (editor: vscode.TextEditor, ranges: LineRange[]) => void;

type PickItem = vscode.QuickPickItem & { choice?: 'apply' | 'cancel'; file?: FileItem };

// PlanPreviews shows a plan before it is applied: each new file as a read-only document of the
// `spellbook-plan` scheme, and over it a QuickPick that holds the plan's title, Apply, Cancel
// and the file list (moving onto a file shows it). A changed file shows as VS Code's diff view:
// its text on disk (a `spellbook-plan-before` document) against the planned text. It is the
// provider of those documents.
export class PlanPreviews implements vscode.TextDocumentContentProvider {
  // the texts of the plans being shown, by scheme and URI path
  private readonly texts = new Map<string, string>();

  constructor(private readonly look: PlaceholderLook) {}

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(textKey(uri.scheme, uri.path)) ?? '';
  }

  // showPlan shows the plan and answers `apply` or `cancel` (Esc, or the pick closed):
  // 1. its new files become preview documents, a changed file's disk text and planned text two
  //    (read under `root`), the first file shown at once;
  // 2. the pick asks; moving onto a file shows it;
  // 3. either way the preview tabs close and the plan's texts are forgotten.
  // Example: a command plan opens cancel_table.py coloured as Python under
  // `spellbook: command · 2 files · 3 holes to fill`; a fix plan opens close_table.py as a diff.
  async showPlan(plan: Plan, taken: Values = {}, settled: Values = {}, root?: vscode.Uri): Promise<'apply' | 'cancel'> {
    const keys = await this.remember(plan, root);
    try {
      const first = [...changedFiles(plan), ...newFiles(plan)][0];
      if (first) await this.showFile(plan, first.path);
      return await this.asked(plan, taken, settled);
    } finally {
      await closePreviews(plan.id);
      for (const key of keys) this.texts.delete(key);
    }
  }

  // remember keeps the texts the preview documents show, and answers their keys.
  private async remember(plan: Plan, root: vscode.Uri | undefined): Promise<string[]> {
    const keys: string[] = [];
    const keep = (scheme: string, path: string, text: string): void => {
      const key = textKey(scheme, planUriPath(plan.id, path));
      this.texts.set(key, text);
      keys.push(key);
    };
    for (const file of [...changedFiles(plan), ...newFiles(plan)]) keep(PLAN_SCHEME, file.path, file.text);
    for (const file of changedFiles(plan)) keep(BEFORE_SCHEME, file.path, await diskText(root, file.path));
    return keys;
  }

  private async showFile(plan: Plan, path: string): Promise<void> {
    const uri = vscode.Uri.from({ scheme: PLAN_SCHEME, path: planUriPath(plan.id, path) });
    if (!this.texts.has(textKey(uri.scheme, uri.path))) return;
    if (changedFiles(plan).some(file => file.path === path)) return this.showDiff(plan, path, uri);
    const editor = await vscode.window.showTextDocument(uri, { preview: true, preserveFocus: true });
    this.look(editor, placeholderRanges(plan, path, row => editor.document.lineAt(Math.min(row, editor.document.lineCount - 1)).text));
  }

  // showDiff opens the file's diff: disk on the left, the plan on the right, both read only.
  private async showDiff(plan: Plan, path: string, planned: vscode.Uri): Promise<void> {
    const before = planned.with({ scheme: BEFORE_SCHEME });
    await vscode.commands.executeCommand('vscode.diff', before, planned, `${path} (the plan)`, { preview: true, preserveFocus: true });
  }

  private asked(plan: Plan, taken: Values, settled: Values): Promise<'apply' | 'cancel'> {
    const pick = vscode.window.createQuickPick<PickItem>();
    pick.title = planTitle(plan, taken, settled);
    pick.placeholder = plan.fix ? 'Apply changes the files; the arrows show each file' : 'Apply writes the files; the arrows show each file';
    pick.ignoreFocusOut = true;
    pick.items = pickItems(plan);
    return new Promise(resolve => {
      let answer: 'apply' | 'cancel' = 'cancel';
      pick.onDidChangeActive(([item]) => item?.file && void this.showFile(plan, item.file.path));
      pick.onDidAccept(() => {
        const choice = pick.activeItems[0]?.choice;
        if (!choice) return;
        answer = choice;
        pick.hide();
      });
      pick.onDidHide(() => {
        pick.dispose();
        resolve(answer);
      });
      pick.show();
    });
  }
}

function pickItems(plan: Plan): PickItem[] {
  return [
    { label: '$(check) Apply', description: plan.fix ? 'change and write the files' : 'write the files', choice: 'apply' },
    { label: '$(close) Cancel', choice: 'cancel' },
    { label: 'files', kind: vscode.QuickPickItemKind.Separator },
    ...fileItems(plan).map(file => ({ label: file.label, description: file.description, file })),
  ];
}

// closePreviews closes every tab showing a preview document of the plan.
async function closePreviews(planId: string): Promise<void> {
  const mine = (uri: vscode.Uri): boolean => uri.scheme === PLAN_SCHEME && uri.path.startsWith(`/${planId}/`);
  const tabs = vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(tab =>
    (tab.input instanceof vscode.TabInputText && mine(tab.input.uri))
    || (tab.input instanceof vscode.TabInputTextDiff && mine(tab.input.modified)));
  if (tabs.length > 0) await vscode.window.tabGroups.close(tabs);
}

// the key a document's text is kept under
function textKey(scheme: string, path: string): string {
  return `${scheme}:${path}`;
}

// diskText is the file's text on disk under the root; empty when it cannot be read.
async function diskText(root: vscode.Uri | undefined, path: string): Promise<string> {
  if (!root) return '';
  try {
    return new TextDecoder().decode(await vscode.workspace.fs.readFile(vscode.Uri.joinPath(root, ...path.split('/'))));
  } catch {
    return '';
  }
}
