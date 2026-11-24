import * as vscode from 'vscode';
import { conjuredLines } from './marks';

// registerConjuredMarks draws a gutter icon and a green overview-ruler tick on every line that
// carries a conjured change's mark, apart from the Problems panel's own look; it redraws when
// diagnostics change or an editor comes into view.
export function registerConjuredMarks(context: vscode.ExtensionContext): void {
  const decoration = vscode.window.createTextEditorDecorationType({
    gutterIconPath: vscode.Uri.joinPath(context.extensionUri, 'media', 'conjured.svg'),
    gutterIconSize: 'contain',
    overviewRulerColor: new vscode.ThemeColor('charts.green'),
    overviewRulerLane: vscode.OverviewRulerLane.Left,
  });
  const redraw = () => vscode.window.visibleTextEditors.forEach(editor => drawOn(editor, decoration));
  context.subscriptions.push(
    decoration,
    vscode.languages.onDidChangeDiagnostics(redraw),
    vscode.window.onDidChangeVisibleTextEditors(redraw),
  );
  redraw();
}

function drawOn(editor: vscode.TextEditor, decoration: vscode.TextEditorDecorationType): void {
  const diagnostics = vscode.languages.getDiagnostics(editor.document.uri)
    .map(d => ({ source: d.source, code: d.code, line: d.range.start.line }));
  const ranges = conjuredLines(diagnostics).map(line => new vscode.Range(line, 0, line, 0));
  editor.setDecorations(decoration, ranges);
}
