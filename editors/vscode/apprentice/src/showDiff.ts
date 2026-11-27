import * as vscode from 'vscode';
import { BEFORE_SCHEME, type ShownDiff } from './diff';

// BeforeTexts shows *Show diff* (`sorcerer/showDiff`): VS Code's diff view of the file's text
// before the task (left, read only, an `apprentice-before` document) against the live buffer
// (right, unsaved edits and all, editable), in a tab of its own. It is the provider of the left
// sides; each diff shown gets its own URI, so a later task's diff never shows an older text.
export class BeforeTexts implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly texts = new Map<string, string>();
  private readonly registration = vscode.workspace.registerTextDocumentContentProvider(BEFORE_SCHEME, this);
  private readonly shown: ShownDiff[] = [];

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(uri.toString()) ?? '';
  }

  async show(diff: ShownDiff): Promise<void> {
    const live = vscode.Uri.parse(diff.uri);
    // the file's own path, so VS Code colours the left side as the file's language
    const before = live.with({ scheme: BEFORE_SCHEME, query: String(this.shown.length + 1) });
    this.texts.set(before.toString(), diff.before);
    this.shown.push(diff);
    await vscode.commands.executeCommand('vscode.diff', before, live, diff.title, { preview: false });
  }

  // everShown is every diff shown, for the end-to-end tests.
  everShown(): ShownDiff[] {
    return this.shown;
  }

  dispose(): void {
    this.registration.dispose();
  }
}
