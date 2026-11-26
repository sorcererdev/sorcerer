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
  holeStatus(): string | undefined;
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
  return rowOfText(doc.getText(), text);
}

function rowOfText(whole: string, text: string): number {
  const row = whole.split(/\r?\n/).findIndex(line => line.includes(text));
  assert.ok(row >= 0, `${text} is in the file`);
  return row;
}

// spellbook diagnostics of URI
function spellbookMarks(uri: vscode.Uri): vscode.Diagnostic[] {
  return vscode.languages.getDiagnostics(uri).filter(d => d.source === 'spellbook');
}

// whether a tab still shows a plan preview (a document, or the right side of a diff)
function previewOpen(): boolean {
  return vscode.window.tabGroups.all.some(g => g.tabs.some(t =>
    (t.input instanceof vscode.TabInputText && t.input.uri.scheme === 'spellbook-plan')
    || (t.input instanceof vscode.TabInputTextDiff && t.input.modified.scheme === 'spellbook-plan')));
}

// applyPlan answers the plan's pick with its first item (Apply), then waits for the command
// DONE to end. The pick opens only after the preview is shown, and an accept sent before it
// opens is lost, so the accept is sent again until the preview closes (the pick has answered).
// Each stage fails on its own, naming where it stopped.
async function applyPlan(done: Thenable<unknown>, said: [string, string][]): Promise<void> {
  let ended = false;
  const command = Promise.resolve(done).finally(() => { ended = true; });
  command.catch(() => undefined);
  let accepts = 0;
  const deadline = Date.now() + 30_000;
  while (previewOpen() && !ended) {
    if (Date.now() > deadline) assert.fail(`the plan's pick did not answer ${accepts} accepts\nsaid: ${JSON.stringify(said)}`);
    await vscode.commands.executeCommand('workbench.action.acceptSelectedQuickOpenItem');
    accepts += 1;
    await new Promise(r => setTimeout(r, 500));
  }
  if (accepts > 1) console.log(`the plan's pick answered after ${accepts} accepts`);
  await until('the command to end after Apply', () => (ended ? true : undefined), 60_000)
    .catch(err => { throw new Error(`${String(err)}\nsaid: ${JSON.stringify(said)}`); });
  await command;
}

// The suite: run by the ubuntu and windows CI jobs and by hand (npm run test:e2e).
describe('spellbook', () => {
  let api: Api;
  before(async () => {
    api = await vscode.extensions.getExtension('sorcererdev.spellbook')!.activate() as Api;
  });

  it('Check puts the finding of a file not open in Problems', async () => {
    await vscode.commands.executeCommand('spellbook.check');
    const uri = vscode.Uri.file(PLANTED);
    const row = rowOfText(readFileSync(PLANTED, 'utf8'), 'import sqlite3');
    const found = await until('the finding from Check', () => {
      const marks = spellbookMarks(uri);
      return marks.length > 0 ? marks : undefined;
    });
    assert.deepEqual(found.map(d => [d.range.start.line, d.code, d.severity]), [[row, 'commands-use-ports', vscode.DiagnosticSeverity.Error]]);
  });

  it('the server marks an open file, and Check then lists its finding once', async () => {
    const uri = vscode.Uri.file(PLANTED);
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);
    const row = rowOf(doc, 'import sqlite3');
    // opening the file takes it out of Check's list; the server's own finding comes instead
    await until('the server\'s finding on the import', () => {
      const found = spellbookMarks(uri).filter(d => d.range.start.line === row);
      return found.length === 1 ? found : undefined;
    }, 60_000);
    await vscode.commands.executeCommand('spellbook.check');
    const marks = spellbookMarks(uri).filter(d => d.code === 'commands-use-ports');
    assert.deepEqual(marks.map(d => d.range.start.line), [row], 'one finding, not two');
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
    await applyPlan(done, said);
    assert.deepEqual(asked, ['command']);
    assert.deepEqual(said, []);
    assert.ok(existsSync(WRITTEN) && existsSync(join(COMMANDS, 'cancel_table__test.py')), 'both files are written');
    assert.ok(!previewOpen(), 'the preview tabs are closed');

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

  it('after New the holes are snippet placeholders: type, Tab, and the status follows', async () => {
    const editor = vscode.window.activeTextEditor;
    assert.ok(editor, 'an editor is active');
    assert.equal(editor.document.uri.toString(), vscode.Uri.file(WRITTEN).toString());
    const disk = readFileSync(WRITTEN, 'utf8');
    // the snippet went in over the same text: nothing re-indented
    assert.equal(editor.document.getText().replace(/\r\n/g, '\n'), disk.replace(/\r\n/g, '\n'));
    assert.equal(editor.document.getText(editor.selection), 'todo_field');
    assert.equal(editor.selection.start.line, 9);
    assert.match(api.holeStatus() ?? '', /^hole 1 of 3 · field: its first input · type to replace · Tab next/);

    // VS Code's snippet keys act on the focused window; a test window without the keyboard (a
    // locked screen) moves the selection itself, which is all the session reads
    if (vscode.window.state.focused) {
      await vscode.commands.executeCommand('type', { text: 'id' });
      await vscode.commands.executeCommand('jumpToNextSnippetPlaceholder');
    } else {
      console.log('the window has no keyboard focus: the selection is moved by the test, not by Tab');
      await editor.edit(edit => edit.replace(editor.selection, 'id'), { undoStopBefore: false, undoStopAfter: false });
      editor.selection = new vscode.Selection(9, 8, 9, 14);
    }
    await until('the second hole selected', () => (editor.document.getText(editor.selection) === 'object' ? true : undefined), 5000)
      .catch(err => { throw new Error(`${String(err)}: line 9 ${JSON.stringify(editor.document.lineAt(9).text)}, selection ${JSON.stringify(editor.selection)}, status ${api.holeStatus()}`); });
    assert.match(editor.document.lineAt(9).text, /^ {4}id: object$/);
    await until('the status on hole 2', () => (api.holeStatus()?.startsWith('hole 2 of 3') ? true : undefined));
    assert.equal(readFileSync(WRITTEN, 'utf8'), disk, 'nothing is saved');

    await vscode.commands.executeCommand('leaveSnippet');
    editor.selection = new vscode.Selection(0, 0, 0, 0);
    await until('the session ended', () => (api.holeStatus() === undefined ? true : undefined));
  });

  it('Fix previews the change as a diff, applies it through the editor, and one undo takes it back', async () => {
    const said: [string, string][] = [];
    const asked: string[] = [];
    api.useAsker({
      pickChoice: async (input: { name: string }, items: string[]) => { asked.push(input.name); return items[0]; },
      say: (level: string, text: string) => { said.push([level, text]); },
    });
    const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(PLANTED));
    const editor = await vscode.window.showTextDocument(doc);
    const row = rowOf(doc, 'import sqlite3');
    editor.selection = new vscode.Selection(row, 0, row, 0);
    const before = doc.getText();
    const done = vscode.commands.executeCommand('spellbook.fixHere');
    // the changed file shows as VS Code's diff: its text on disk against the plan
    await until('the diff of the plan', () => vscode.window.tabGroups.all.flatMap(g => g.tabs)
      .find(t => t.input instanceof vscode.TabInputTextDiff && t.input.modified.scheme === 'spellbook-plan'), 60_000)
      .catch(err => { throw new Error(`${String(err)}\nsaid: ${JSON.stringify(said)}`); });
    await applyPlan(done, said);
    assert.deepEqual(said, []);
    assert.deepEqual(asked, ['port']);
    await until('the fix landed in the editor', () => (doc.getText().includes('import sqlite3') ? undefined : true), 30_000);
    assert.ok(doc.isDirty, 'the change is in the editor, unsaved');
    assert.equal(readFileSync(PLANTED, 'utf8').replace(/\r\n/g, '\n'), before.replace(/\r\n/g, '\n'), 'nothing is saved');
    // `undo` acts on the focused window; a test window without the keyboard (a locked screen)
    // ignores it, and the by-hand walk covers that step
    if (!vscode.window.state.focused) {
      console.log('the window has no keyboard focus: the undo step is left to the by-hand walk');
      return;
    }
    await vscode.window.showTextDocument(doc);
    await vscode.commands.executeCommand('undo');
    await until('the undo', () => (doc.getText() === before ? true : undefined), 5000);
  });
});
