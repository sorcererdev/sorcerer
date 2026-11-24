import * as vscode from 'vscode';
import { holeRanges } from './marks';
import type { LineRange } from './plan';

// holeDecoration is the hole look: a `◇` in the gutter, a blue tick in the overview ruler, and
// the placeholder tinted as VS Code tints a snippet's tab stop, so a hole reads as what it is: a
// tab stop waiting.
export function holeDecoration(context: vscode.ExtensionContext): vscode.TextEditorDecorationType {
  const decoration = vscode.window.createTextEditorDecorationType({
    gutterIconPath: vscode.Uri.joinPath(context.extensionUri, 'media', 'hole.svg'),
    gutterIconSize: 'contain',
    backgroundColor: new vscode.ThemeColor('editor.snippetTabstopHighlightBackground'),
    overviewRulerColor: new vscode.ThemeColor('charts.blue'),
    overviewRulerLane: vscode.OverviewRulerLane.Left,
  });
  context.subscriptions.push(decoration);
  return decoration;
}

// registerHoleMarks draws the hole look on every hole mark of the visible editors, apart from
// the Problems panel's own look; it redraws when diagnostics change or an editor comes into view.
export function registerHoleMarks(context: vscode.ExtensionContext, decoration: vscode.TextEditorDecorationType): void {
  const redraw = () => vscode.window.visibleTextEditors
    .filter(editor => editor.document.uri.scheme === 'file')
    .forEach(editor => drawOn(editor, decoration));
  context.subscriptions.push(
    vscode.languages.onDidChangeDiagnostics(redraw),
    vscode.window.onDidChangeVisibleTextEditors(redraw),
  );
  redraw();
}

// placeholderLook draws the hole look on a plan preview's placeholders.
export function placeholderLook(decoration: vscode.TextEditorDecorationType): (editor: vscode.TextEditor, ranges: LineRange[]) => void {
  return (editor, ranges) => editor.setDecorations(decoration, ranges.map(r => new vscode.Range(r.line, r.start, r.line, r.end)));
}

function drawOn(editor: vscode.TextEditor, decoration: vscode.TextEditorDecorationType): void {
  const ranges = holeRanges(vscode.languages.getDiagnostics(editor.document.uri).map(d => ({ source: d.source, code: d.code, range: d.range })))
    .map(r => new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character));
  editor.setDecorations(decoration, ranges);
}
