import * as assert from 'node:assert/strict';
import { join } from 'node:path';
import * as vscode from 'vscode';

// until is the value of `look` once it is not undefined, polling for at most `ms`.
async function until<T>(what: string, look: () => T | undefined | Promise<T | undefined>, ms = 20_000): Promise<T> {
  const deadline = Date.now() + ms;
  for (;;) {
    const found = await look();
    if (found !== undefined) return found;
    if (Date.now() > deadline) assert.fail(`timed out waiting for ${what}`);
    await new Promise(r => setTimeout(r, 200));
  }
}

const ws = process.env.SPELLBOOK_E2E_WS!;
const PLANTED = join(ws, 'src', 'bistro', 'booking', 'commands', 'create_booking.py');

// the row (from 0) of the first line of DOC that contains TEXT
function rowOf(doc: vscode.TextDocument, text: string): number {
  const row = doc.getText().split(/\r?\n/).findIndex(line => line.includes(text));
  assert.ok(row >= 0, `${text} is in ${doc.uri.fsPath}`);
  return row;
}

// spellbook diagnostics of URI
function spellbookMarks(uri: vscode.Uri): vscode.Diagnostic[] {
  return vscode.languages.getDiagnostics(uri).filter(d => d.source === 'spellbook');
}

// The suite: run by the ubuntu and windows CI jobs and by hand (npm run test:e2e).
describe('spellbook', () => {
  before(async () => {
    await vscode.extensions.getExtension('sorcererdev.spellbook')!.activate();
  });

  it('the server starts and its rules mark the planted import of an open file', async () => {
    const uri = vscode.Uri.file(PLANTED);
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);
    const row = rowOf(doc, 'import sqlite3');
    await until('the finding on the import', () => {
      const found = spellbookMarks(uri).filter(d => d.range.start.line === row);
      return found.length > 0 ? found : undefined;
    }, 60_000);
  });
});
