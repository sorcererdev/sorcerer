import * as vscode from 'vscode';
import { type FileItem, fileItems, type LineRange, newFiles, type Plan, placeholderRanges, planTitle, planUriPath } from './plan';

// The scheme of the plan's preview documents: virtual, so they are read-only, and each path
// ends in the planned file's name, so VS Code colours it as that file's language.
export const PLAN_SCHEME = 'spellbook-plan';

// Draws the placeholder look on a preview's ranges (the hole look, holeMarks.ts).
export type PlaceholderLook = (editor: vscode.TextEditor, ranges: LineRange[]) => void;

type PickItem = vscode.QuickPickItem & { choice?: 'apply' | 'cancel'; file?: FileItem };

// PlanPreviews shows a plan before it is applied: each new file as a read-only document of the
// `spellbook-plan` scheme, and over it a QuickPick that holds the plan's title, Apply, Cancel
// and the file list (moving onto a file shows it). It is the provider of those documents.
export class PlanPreviews implements vscode.TextDocumentContentProvider {
  // the texts of the plans being shown, by URI path
  private readonly texts = new Map<string, string>();

  constructor(private readonly look: PlaceholderLook) {}

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(uri.path) ?? '';
  }

  // showPlan shows the plan and answers `apply` or `cancel` (Esc, or the pick closed):
  // 1. its new files become preview documents, the first shown at once;
  // 2. the pick asks; moving onto a file shows it;
  // 3. either way the preview tabs close and the plan's texts are forgotten.
  // Example: a command plan opens cancel_table.py coloured as Python under
  // `spellbook: command · 2 files · 3 holes to fill`.
  async showPlan(plan: Plan): Promise<'apply' | 'cancel'> {
    for (const file of newFiles(plan)) this.texts.set(planUriPath(plan.id, file.path), file.text);
    try {
      const first = newFiles(plan)[0];
      if (first) await this.showFile(plan, first.path);
      return await this.asked(plan);
    } finally {
      await closePreviews(plan.id);
      for (const file of newFiles(plan)) this.texts.delete(planUriPath(plan.id, file.path));
    }
  }

  private async showFile(plan: Plan, path: string): Promise<void> {
    const uri = vscode.Uri.from({ scheme: PLAN_SCHEME, path: planUriPath(plan.id, path) });
    if (!this.texts.has(uri.path)) return;
    const editor = await vscode.window.showTextDocument(uri, { preview: true, preserveFocus: true });
    this.look(editor, placeholderRanges(plan, path, row => editor.document.lineAt(Math.min(row, editor.document.lineCount - 1)).text));
  }

  private asked(plan: Plan): Promise<'apply' | 'cancel'> {
    const pick = vscode.window.createQuickPick<PickItem>();
    pick.title = planTitle(plan);
    pick.placeholder = 'Apply writes the files; the arrows show each file';
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
    { label: '$(check) Apply', description: 'write the files', choice: 'apply' },
    { label: '$(close) Cancel', choice: 'cancel' },
    { label: 'files', kind: vscode.QuickPickItemKind.Separator },
    ...fileItems(plan).map(file => ({ label: file.label, description: file.description, file })),
  ];
}

// closePreviews closes every tab showing a preview document of the plan.
async function closePreviews(planId: string): Promise<void> {
  const tabs = vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(tab =>
    tab.input instanceof vscode.TabInputText
    && tab.input.uri.scheme === PLAN_SCHEME
    && tab.input.uri.path.startsWith(`/${planId}/`));
  if (tabs.length > 0) await vscode.window.tabGroups.close(tabs);
}
