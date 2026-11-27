import * as vscode from 'vscode';
import { forDocument, type WorkingUnit } from './working';

// Working draws the units a server is working on as grey text at the end of each one's first
// line, in the editors showing them, and takes it away when the server says it is done. VS Code
// moves a decoration with the text as the person types, so the text stays on its line; it is
// drawn again when an editor opens or comes back into view.
export class Working implements vscode.Disposable {
  private units: WorkingUnit[] = [];
  private readonly everShown: WorkingUnit[] = [];
  private readonly type = vscode.window.createTextEditorDecorationType({});
  private readonly redraw = vscode.window.onDidChangeVisibleTextEditors(() => this.draw());

  // show replaces what is drawn with these units; none clears it.
  show(units: WorkingUnit[]): void {
    this.units = units;
    this.everShown.push(...units);
    this.draw();
  }

  clear(): void {
    this.show([]);
  }

  // drawn is what is drawn now, for the end-to-end tests.
  drawn(): WorkingUnit[] {
    return this.units;
  }

  // everDrawn is every unit drawn since the start, for the end-to-end tests: a fast server
  // draws and clears between two looks.
  everDrawn(): WorkingUnit[] {
    return this.everShown;
  }

  dispose(): void {
    this.redraw.dispose();
    this.type.dispose();
  }

  private draw(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      editor.setDecorations(this.type, forDocument(this.units, editor.document.uri.toString()).flatMap(unit => decoration(editor, unit)));
    }
  }
}

// decoration is the text after the unit's first line, none when the document has no such line.
function decoration(editor: vscode.TextEditor, unit: WorkingUnit): vscode.DecorationOptions[] {
  if (unit.line >= editor.document.lineCount) return [];
  const end = editor.document.lineAt(unit.line).range.end;
  return [{
    range: new vscode.Range(end, end),
    renderOptions: {
      after: { contentText: unit.text, color: new vscode.ThemeColor('editorCodeLens.foreground'), margin: '0 0 0 2em', fontStyle: 'italic' },
    },
  }];
}
