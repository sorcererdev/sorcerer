import { basename } from 'node:path';
import * as vscode from 'vscode';
import { type ApplyWorkspaceEditParams, type ApplyWorkspaceEditRequest, type ApplyWorkspaceEditResult, CancellationToken } from 'vscode-languageclient/node';
import { LAST_CHANGE, nearestLine, NOTHING_TO_UNDO, type Place, placesOf, undoneText } from './lastChange';

// how long after an undo starts the server's edits are that undo, not a change of their own
const UNDO_WINDOW_MS = 5000;

// What undoing asks of the tool: the action titled `title` on its mark (a diagnostic of the
// tool's with code `markCode`) nearest the change, or, with no title, VS Code's own undo in each
// file the change touched.
export interface UndoBy {
  tool: 'conjure' | 'lackey' | 'spellbook';
  title?: string;
  markCode?: string;
  // what to say when the mark or its action is gone
  gone: string;
}

type ServerAction = vscode.Command | vscode.CodeAction;

// LastChange remembers the last change this extension's server landed (its edits through
// `workspace/applyEdit`), says so in the `sorcerer.lastChange` context, and undoes it on
// `ctrl+m u`. A lackey task lands many edits: each one moves the change to its latest place, and
// the task's mark is found nearest it.
export class LastChange {
  private places: Place[] = [];
  private versions = new Map<string, number>();
  private undoingUntil = 0;

  constructor(private readonly by: UndoBy) {}

  // handleApplyEdit is the language client's middleware for `workspace/applyEdit`: the edit is
  // applied as ever, then remembered when it landed.
  readonly handleApplyEdit = async (params: ApplyWorkspaceEditParams, next: ApplyWorkspaceEditRequest.HandlerSignature): Promise<ApplyWorkspaceEditResult> => {
    // the client's next takes the params alone; the token is for its type
    const result = (await next(params, CancellationToken.None)) as ApplyWorkspaceEditResult;
    if (result?.applied) this.landed(placesOf(params.edit as never));
    return result;
  };

  private landed(places: Place[]): void {
    if (places.length === 0) return;
    if (Date.now() < this.undoingUntil) {
      this.undoingUntil = Date.now() + UNDO_WINDOW_MS;
      return;
    }
    this.places = places;
    this.versions = new Map(places.map(place => [place.uri, documentOf(place.uri)?.version ?? -1]));
    void vscode.commands.executeCommand('setContext', LAST_CHANGE, this.by.tool);
  }

  // undo takes the last change back and says whose it was; with none, it says so.
  async undo(): Promise<void> {
    if (this.places.length === 0) {
      vscode.window.setStatusBarMessage(NOTHING_TO_UNDO, 4000);
      return;
    }
    const undone = this.by.title ? await this.undoByAction(this.by.title) : await this.undoInFiles();
    if (!undone) return;
    this.places = [];
    void vscode.commands.executeCommand('setContext', LAST_CHANGE, undefined);
    vscode.window.setStatusBarMessage(undoneText(this.by.tool, basename(vscode.Uri.parse(undone.uri).fsPath), undone.line), 6000);
  }

  // the tool's own undo: the action titled TITLE on the mark nearest the change
  private async undoByAction(title: string): Promise<Place | undefined> {
    for (const place of this.places) {
      const uri = vscode.Uri.parse(place.uri);
      const marks = vscode.languages.getDiagnostics(uri)
        .filter(d => d.source === this.by.tool && codeOf(d) === this.by.markCode)
        .map(d => d.range.start.line);
      const line = nearestLine(marks, place.line);
      if (line === undefined) continue;
      const actions = await vscode.commands.executeCommand<ServerAction[]>(
        'vscode.executeCodeActionProvider', uri, new vscode.Range(line, 0, line, 0));
      const action = (actions ?? []).find(a => a.title === title);
      if (!action) continue;
      this.undoingUntil = Date.now() + UNDO_WINDOW_MS;
      await run(action);
      return { uri: place.uri, line };
    }
    vscode.window.setStatusBarMessage(this.by.gone, 6000);
    return undefined;
  }

  // VS Code's own undo in each file, while the change is still that file's last edit
  private async undoInFiles(): Promise<Place | undefined> {
    const moved = this.places.find(place => documentOf(place.uri)?.version !== this.versions.get(place.uri));
    if (moved) {
      void vscode.window.showWarningMessage(`${this.by.tool}'s last change is no longer the last edit of ${basename(vscode.Uri.parse(moved.uri).fsPath)}: undo it there`);
      return undefined;
    }
    for (const place of this.places) {
      const document = documentOf(place.uri);
      if (!document) continue;
      await vscode.window.showTextDocument(document);
      await vscode.commands.executeCommand('undo');
    }
    return this.places[0];
  }
}

// registerUndoLast makes the command `ctrl+m u` calls when this tool changed the code last (or
// when no tool has).
export function registerUndoLast(context: vscode.ExtensionContext, command: string, last: LastChange): void {
  context.subscriptions.push(vscode.commands.registerCommand(command, () => last.undo()));
}

function documentOf(uri: string): vscode.TextDocument | undefined {
  const wanted = vscode.Uri.parse(uri).toString();
  return vscode.workspace.textDocuments.find(doc => doc.uri.toString() === wanted);
}

function codeOf(d: vscode.Diagnostic): string | undefined {
  const code = typeof d.code === 'object' ? d.code.value : d.code;
  return code === undefined ? undefined : String(code);
}

// the server's actions are code actions that hold a command; a bare command is its own
async function run(action: ServerAction): Promise<void> {
  const command: vscode.Command | undefined = 'kind' in action || typeof action.command === 'object'
    ? (action as vscode.CodeAction).command : (action as vscode.Command);
  if (command) await vscode.commands.executeCommand(command.command, ...(command.arguments ?? []));
}
