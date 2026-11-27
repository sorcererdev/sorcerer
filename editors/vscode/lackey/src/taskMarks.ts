import * as vscode from 'vscode';
import { taskLines } from './marks';

// registerTaskMarks draws a gutter icon (a violet ✦) and a purple overview-ruler tick on the first
// changed line of each file a finished task changed (its mark), apart from the Problems panel's
// own look; it redraws when diagnostics change or an editor comes into view.
export function registerTaskMarks(context: vscode.ExtensionContext): void {
  const decoration = vscode.window.createTextEditorDecorationType({
    gutterIconPath: vscode.Uri.joinPath(context.extensionUri, 'media', 'task.svg'),
    gutterIconSize: 'contain',
    overviewRulerColor: new vscode.ThemeColor('charts.purple'),
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
  const ranges = taskLines(diagnostics).map(line => new vscode.Range(line, 0, line, 0));
  editor.setDecorations(decoration, ranges);
}
