import * as assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
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
const COMMANDS = join(ws, 'src', 'bistro', 'booking', 'commands');
const PLANTED = join(COMMANDS, 'create_booking.py');
const WRITTEN = join(COMMANDS, 'cancel_table.py');

// What the extension's activate returns (see SpellbookApi).
interface Api {
  useAsker(overrides: Record<string, unknown>): void;
}

// The person's answers to Spellbook: New: the command generator, `cancel_table` typed (bistro
// has one app and one service, taken without asking). The plan is shown by the real preview.
function answers(said: [string, string][], asked: string[]): Record<string, unknown> {
  return {
    pickGenerator: async (gs: { id: string }[]) => gs.find(g => g.id === 'command'),
    pickChoice: async (input: { name: string }) => { asked.push(input.name); return undefined; },
    askText: async (input: { name: string }) => { asked.push(input.name); return 'cancel_table'; },
    say: (level: string, text: string) => { said.push([level, text]); },
  };
}

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
  let api: Api;
  before(async () => {
    api = await vscode.extensions.getExtension('sorcererdev.spellbook')!.activate() as Api;
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

  it('New previews the plan as coloured documents, applies it, and marks the holes', async () => {
    const said: [string, string][] = [];
    const asked: string[] = [];
    api.useAsker(answers(said, asked));
    const done = vscode.commands.executeCommand('spellbook.new');
    // the first new file is shown as a read-only preview, coloured as its language
    const preview = await until('the plan preview', () =>
      vscode.window.visibleTextEditors.find(e => e.document.uri.scheme === 'spellbook-plan'), 60_000)
      .catch(err => { throw new Error(`${String(err)}\nsaid: ${JSON.stringify(said)}`); });
    assert.match(preview.document.uri.path, /cancel_table\.py$/);
    assert.equal(preview.document.languageId, 'python');
    assert.match(preview.document.getText(), /class CancelTableInput:/);
    // Apply is the pick's first item
    await vscode.commands.executeCommand('workbench.action.acceptSelectedQuickOpenItem');
    await done;
    assert.deepEqual(asked, ['command']);
    assert.deepEqual(said, []);
    assert.ok(existsSync(WRITTEN) && existsSync(join(COMMANDS, 'cancel_table__test.py')), 'both files are written');
    assert.ok(!vscode.window.tabGroups.all.some(g => g.tabs.some(t =>
      t.input instanceof vscode.TabInputText && t.input.uri.scheme === 'spellbook-plan')), 'the preview tabs are closed');

    const uri = vscode.Uri.file(WRITTEN);
    const holes = await until('the hole marks', () => {
      const found = spellbookMarks(uri).filter(d => d.code === 'hole');
      return found.length === 3 ? found : undefined;
    }, 30_000);
    const lines = readFileSync(WRITTEN, 'utf8').split(/\r?\n/);
    const slices = holes.map(d => [d.range.start.line, lines[d.range.start.line].slice(d.range.start.character, d.range.end.character)]);
    assert.deepEqual(slices.slice(0, 2), [[9, 'todo_field'], [9, 'object']]);
    assert.equal(slices[2][0], 29);
    assert.match(String(slices[2][1]), /^raise NotImplementedError\("hole handle/);
  });
});
