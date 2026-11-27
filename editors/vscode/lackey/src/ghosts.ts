import * as vscode from 'vscode';
import { afterChanges, type Ghost, ghostsIn, ghostText, withGhost } from './ghost';

// Ghosts draws lackey's suggested lines (`sorcerer/ghost`): VS Code has no lines between
// lines, so a ghost is grey text after the line it hangs under (`⤷ if late: ⏎ raise TooLate()`,
// the theme's ghost-text colour), its lines whole on hover. The person's next edit of that line
// takes it away and an edit above moves it (see afterChanges); it is drawn again when an editor
// comes into view.
export class Ghosts implements vscode.Disposable {
  private ghosts: Ghost[] = [];
  private readonly everShown: Ghost[] = [];
  private readonly type = vscode.window.createTextEditorDecorationType({});
  private readonly listeners = [
    vscode.window.onDidChangeVisibleTextEditors(() => this.draw()),
    vscode.workspace.onDidChangeTextDocument(e => this.edited(e)),
  ];

  // show puts a ghost under its line in place of the one there; no lines takes it away.
  show(ghost: Ghost): void {
    this.ghosts = withGhost(this.ghosts, ghost);
    if (ghost.lines.length > 0) this.everShown.push(ghost);
    this.draw();
  }

  clear(): void {
    this.ghosts = [];
    this.draw();
  }

  // drawn is the ghosts shown now, for the end-to-end tests.
  drawn(): Ghost[] {
    return this.ghosts;
  }

  // everDrawn is every ghost shown since the start, for the end-to-end tests.
  everDrawn(): Ghost[] {
    return this.everShown;
  }

  dispose(): void {
    for (const listener of this.listeners) listener.dispose();
    this.type.dispose();
  }

  private edited(e: vscode.TextDocumentChangeEvent): void {
    const uri = e.document.uri.toString();
    const here = ghostsIn(this.ghosts, uri);
    if (here.length === 0 || e.contentChanges.length === 0) return;
    const changes = e.contentChanges.map(change => ({
      startLine: change.range.start.line,
      endLine: change.range.end.line,
      endCharacter: change.range.end.character,
      text: change.text,
    }));
    this.ghosts = [...this.ghosts.filter(ghost => ghost.uri !== uri), ...afterChanges(here, changes)];
    this.draw();
  }

  private draw(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      const here = ghostsIn(this.ghosts, editor.document.uri.toString());
      editor.setDecorations(this.type, here.flatMap(ghost => decoration(editor, ghost)));
    }
  }
}

// decoration is the ghost after its line, none when the document has no such line.
function decoration(editor: vscode.TextEditor, ghost: Ghost): vscode.DecorationOptions[] {
  if (ghost.line >= editor.document.lineCount) return [];
  const end = editor.document.lineAt(ghost.line).range.end;
  const hover = new vscode.MarkdownString().appendText('lackey suggests:').appendCodeblock(ghost.lines.join('\n'), editor.document.languageId);
  return [{
    range: new vscode.Range(end, end),
    hoverMessage: hover,
    renderOptions: {
      after: { contentText: ghostText(ghost.lines), color: new vscode.ThemeColor('editorGhostText.foreground'), margin: '0 0 0 2em', fontStyle: 'italic' },
    },
  }];
}
