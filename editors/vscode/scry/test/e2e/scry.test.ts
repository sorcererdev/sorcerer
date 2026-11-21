import * as assert from 'node:assert/strict';
import { join } from 'node:path';
import * as vscode from 'vscode';

// Run by hand and in no CI job (it needs a VS Code download): npm run test:e2e.
describe('scry (fake server)', () => {
  it('a save puts the server\'s finding on its line, source scry, within 20 s', async () => {
    const uri = vscode.Uri.file(join(process.env.SCRY_E2E_WS!, 'a.py'));
    const doc = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(doc);
    // a save of an unchanged file sends nothing: change it first
    await editor.edit(b => b.insert(new vscode.Position(1, doc.lineAt(1).text.length), ' '));
    assert.ok(await doc.save(), 'the document saved');
    const deadline = Date.now() + 20_000;
    let found: vscode.Diagnostic[] = [];
    while (Date.now() < deadline) {
      found = vscode.languages.getDiagnostics(uri).filter(d => d.source === 'scry');
      if (found.length > 0) break;
      await new Promise(r => setTimeout(r, 200));
    }
    assert.equal(found.length, 1, 'one diagnostic from scry');
    assert.equal(found[0].code, 'bug');
    assert.equal(found[0].severity, vscode.DiagnosticSeverity.Error);
    assert.equal(found[0].range.start.line, 1);
    assert.match(found[0].message, /f adds two, not one\./);
  });
});
