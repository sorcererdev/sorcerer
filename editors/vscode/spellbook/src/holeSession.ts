import * as vscode from 'vscode';
import { doneText, messageText, type Range, shifted, snippetFor, type Stop, stopAt } from './holes';

// The running session, at most one: a new one ends the old.
let current: HoleSession | undefined;

// holeStatus is the session's status line, or undefined when no session runs (for the
// end-to-end tests).
export function holeStatus(): string | undefined {
  return current?.status();
}

// startHoleSession turns the editor's holes into VS Code's own snippet placeholders and follows
// them with a status line; false when the snippet could not go in (the document changed
// meanwhile), and then no session runs.
// 1. one snippet over the lines from the first hole to the last, the holes as `${n:…}`, put over
//    those same lines: the first placeholder is selected, Tab and Shift+Tab move, Esc leaves;
// 2. the holes' ranges follow each edit, and each selection says which hole it is in;
// 3. a selection outside every hole ends the session and says so (and where the other files'
//    holes are).
// Example: cancel_table.py's three holes: `hole 1 of 3 · field: its first input · …`.
export async function startHoleSession(editor: vscode.TextEditor, mine: Stop[], others: Map<string, number>, conjureActive: boolean): Promise<boolean> {
  current?.end(false);
  const lines = Array.from({ length: editor.document.lineCount }, (_, i) => editor.document.lineAt(i).text);
  const { region, snippet } = snippetFor(lines, mine);
  const replaced = new vscode.Range(region.startLine, 0, region.endLine, lines[region.endLine].length);
  // keepWhitespace (VS Code 1.9x and later; older ones ignore it): the region's lines keep their
  // own indent instead of taking the first line's
  const options = { undoStopBefore: true, undoStopAfter: false, keepWhitespace: true };
  const session = new HoleSession(editor, mine, others, conjureActive);
  current = session;
  const inserted = await editor.insertSnippet(new vscode.SnippetString(snippet), replaced, options);
  if (!inserted) {
    session.end(false);
    return false;
  }
  session.listen();
  return true;
}

// HoleSession is one file's walk through its holes: their ranges as they are now, the status
// bar item, and the listeners that keep them current.
class HoleSession {
  private ranges: Range[];
  private readonly item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  private readonly listeners: vscode.Disposable[] = [];

  constructor(private readonly editor: vscode.TextEditor, private readonly stops: Stop[],
    private readonly others: Map<string, number>, private readonly conjureActive: boolean) {
    this.ranges = stops.map(stop => stop.range);
    this.say(0);
  }

  status(): string {
    return this.item.text;
  }

  listen(): void {
    const doc = this.editor.document;
    this.listeners.push(
      vscode.workspace.onDidChangeTextDocument(e => {
        if (e.document === doc) this.ranges = shifted(this.ranges, e.contentChanges);
      }),
      vscode.window.onDidChangeTextEditorSelection(e => {
        if (e.textEditor.document === doc) this.follow(e.selections[0]);
      }),
      vscode.workspace.onDidCloseTextDocument(closed => closed === doc && this.end(false)),
    );
  }

  // follow says the message of the hole that holds the selection, or ends the session when
  // none does.
  private follow(selection: vscode.Selection): void {
    const at = stopAt(this.ranges, selection.active);
    if (at < 0 || stopAt(this.ranges, selection.anchor) !== at) this.end(true);
    else this.say(at);
  }

  private say(at: number): void {
    this.item.text = messageText(at + 1, this.stops.length, this.stops[at], this.conjureActive);
    this.item.show();
  }

  // end stops listening and takes the item away; `said` tells the person the holes are done.
  end(said: boolean): void {
    if (current === this) current = undefined;
    for (const listener of this.listeners.splice(0)) listener.dispose();
    this.item.dispose();
    if (said) vscode.window.setStatusBarMessage(doneText(this.others), 10_000);
  }
}
