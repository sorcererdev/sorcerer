import * as assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as vscode from 'vscode';
import type { LackeyApi } from '../../src/extension';

const STUB = 'def total(cart):\n    return 1\n\n\ndef tax(cart):\n    pass\n';
const DONE = 'def total(cart):\n    return 1\n\n\ndef tax(cart):\n    return total(cart) * 0.2\n';

// until is the value of `look` once it is not undefined, polling for at most `ms`.
async function until<T>(what: string, look: () => T | undefined | Promise<T | undefined>, ms = 30_000): Promise<T> {
  const deadline = Date.now() + ms;
  for (;;) {
    const found = await look();
    if (found !== undefined) return found;
    if (Date.now() > deadline) assert.fail(`timed out waiting for ${what}`);
    await new Promise(r => setTimeout(r, 200));
  }
}

// the code action on LINE (from 0) of URI whose title starts with TITLE
async function actionStarting(uri: vscode.Uri, title: string, line: number): Promise<vscode.CodeAction | undefined> {
  // a request made before the server is up is cancelled: ask again
  const actions = await Promise.resolve(vscode.commands.executeCommand<vscode.CodeAction[]>(
    'vscode.executeCodeActionProvider', uri, new vscode.Range(line, 0, line, 0))).catch(() => undefined);
  return (actions ?? []).find(a => a.title.startsWith(title));
}

// press runs the key's command with the cursor at the start of LINE of the document
async function press(command: string, doc: vscode.TextDocument, line: number): Promise<void> {
  const editor = await vscode.window.showTextDocument(doc, { viewColumn: vscode.ViewColumn.One });
  editor.selection = new vscode.Selection(line, 0, line, 0);
  await vscode.commands.executeCommand(command);
}

// What the person "answers" to the server's questions with buttons (a permission is one), in
// order; what VS Code was asked to say is kept, for the assertions and a failure message.
const answers: (string | undefined)[] = [];
const said: string[] = [];

function answerQuestionsFromTheQueue(): void {
  for (const name of ['showInformationMessage', 'showWarningMessage', 'showErrorMessage'] as const) {
    (vscode.window as unknown as Record<string, unknown>)[name] = (text: string, ...items: unknown[]) => {
      said.push(text);
      const buttons = items.filter((item): item is { title: string } => typeof (item as { title?: unknown })?.title === 'string');
      if (buttons.length === 0) return Promise.resolve(undefined);
      const answer = answers.shift();
      return Promise.resolve(buttons.find(button => button.title === answer));
    };
  }
}

function state(ws: string, file: string): string {
  try {
    return readFileSync(join(ws, '.sorcerer', 'state', 'lackey', file), 'utf8');
  } catch {
    return '';
  }
}

// lines of the fake claude's log: each tool's result, each permission it asked for
function fakeLog(): { permission?: unknown; command?: string }[] {
  try {
    return readFileSync(process.env.FAKE_LACKEY_LOG!, 'utf8').split('\n').filter(line => line !== '').map(line => JSON.parse(line));
  } catch {
    return [];
  }
}

function why(ws: string, uri: vscode.Uri): string {
  return `tasks: ${state(ws, 'tasks.jsonl')}\nmarks: ${JSON.stringify(vscode.languages.getDiagnostics(uri).map(d => [d.range.start.line, d.code, d.message]))}\nsaid: ${JSON.stringify(said)}\nfake: ${JSON.stringify(fakeLog())}`;
}

// The fake-claude suite: run by the ubuntu and windows CI jobs and by hand (npm run test:e2e).
describe('lackey (fake claude)', () => {
  const ws = process.env.LACKEY_E2E_WS!;
  const uri = vscode.Uri.file(join(ws, 'cart.py'));
  let api: LackeyApi;
  let doc: vscode.TextDocument;

  before(async () => {
    answerQuestionsFromTheQueue();
    api = await vscode.extensions.getExtension('sorcererdev.lackey')!.activate() as LackeyApi;
    doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);
  });

  it('a stub is offered, done in the buffer through every medium, diffed, journaled and undone', async () => {
    writeFileSync(process.env.LACKEY_E2E_SCRIPT!, JSON.stringify({
      // the first write is refused with the buffer's text (the stub is unsaved and the agent
      // read the disk); the agent writes again on it, and that one lands
      steps: [
        { tool: 'write_file', arguments: { path: 'cart.py', text: DONE } },
        { tool: 'write_file', arguments: { path: 'cart.py', text: DONE } },
        { tool: 'ghost', arguments: { file: 'cart.py', line: 6, lines: ['    # rounded to cents?'] } },
        { tool: 'note', arguments: { title: 'Why tax', text: 'Tax is 20% (cart.py:6).' } },
        { tool: 'mark', arguments: { file: 'cart.py', line: 1, text: 'total is a stub too' } },
        { bash: 'uv run pytest -q' },
        { bash: 'uv run pytest -q' },
        { tool: 'notify', arguments: { text: 'tax is 20%' } },
      ],
      cost: 0.05,
      result: 'done',
    }));
    // the first permission question is closed unanswered, the second allowed once
    answers.push(undefined, 'Allow once');
    const editor = await vscode.window.showTextDocument(doc);
    await editor.edit(edit => edit.insert(doc.lineAt(doc.lineCount - 1).range.end, '\n\ndef tax(cart):\n    pass\n'));
    assert.equal(doc.getText(), STUB);

    // an empty body is offered by rule (no Jev), on its def line
    await until('the offer', () => actionStarting(uri, 'lackey: do this', 4), 60_000)
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}`); });
    await press('lackey.keys.do', doc, 4);

    await until('the task\'s text in the buffer', () => (doc.getText() === DONE ? true : undefined), 60_000)
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}`); });
    assert.equal(readFileSync(uri.fsPath, 'utf8'), 'def total(cart):\n    return 1\n', 'nothing was saved: the disk is as committed');
    // the task's line says it works from the start, then what it does; the writing ghost moves
    // to the line written once the server hears the edit landed, which is after VS Code has the
    // text: wait for it in what was ever drawn (a fast task draws and clears it)
    const everDrawn = () => api.everDrawn().map(u => [vscode.Uri.parse(u.uri).toString(), u.line, u.text] as const);
    const writing = [uri.toString(), 5, '⟳ lackey: writing…'] as const;
    const textAt = Date.now();
    await until('the writing ghost', () => (everDrawn().some(u => u[0] === writing[0] && u[1] === writing[1] && u[2] === writing[2]) ? true : undefined), 10_000)
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}\n${JSON.stringify(everDrawn())}`); });
    console.log(`the writing ghost came ${Date.now() - textAt} ms after the text`);
    await until('the done message', () => (said.some(text => text.startsWith('lackey: done')) ? true : undefined))
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}`); });
    assert.ok(said.includes('lackey: tax is 20%'), JSON.stringify(said));
    // first on the def line (4 from 0), only ever on cart.py, never the status line's title and
    // spend, and cleared when the task is done
    const drawn = everDrawn();
    assert.deepEqual(drawn[0], [uri.toString(), 4, '⟳ lackey: working…'], JSON.stringify(drawn));
    const writingAt = drawn.findIndex(u => u[1] === 5 && u[2] === writing[2]);
    assert.ok(writingAt > 0, JSON.stringify(drawn));
    assert.ok(drawn.slice(0, writingAt).every(u => u[1] === 4), JSON.stringify(drawn));
    assert.ok(drawn.slice(writingAt).every(u => u[1] === 5), JSON.stringify(drawn));
    assert.ok(drawn.every(u => u[0] === uri.toString() && u[2].startsWith('⟳ lackey: ') && !u[2].includes(' · ')), JSON.stringify(drawn));
    await until('the working line cleared', () => (api.drawn().length === 0 ? true : undefined), 10_000);

    // the ghost hung under line 6 (5 from 0), the note opened beside the code
    // stored as VS Code spells the document (on Windows the server's drive letter is not)
    assert.deepEqual(api.ghosts().map(g => g.uri), [uri.toString()]);
    assert.deepEqual(api.everGhosts().map(g => [vscode.Uri.parse(g.uri).toString(), g.line, g.lines]), [[uri.toString(), 5, ['    # rounded to cents?']]]);
    const note = await until('the note beside', () => vscode.window.visibleTextEditors.find(e => e.document.uri.fsPath.includes('notes')));
    assert.match(note.document.getText(), /^# Why tax/);
    assert.notEqual(note.viewColumn, vscode.ViewColumn.One);

    // permissions fail closed: the closed question denied the command, the answered one allowed it
    const permissions = fakeLog().filter(entry => entry.permission !== undefined).map(entry => JSON.stringify(entry.permission));
    assert.equal(permissions.length, 2, why(ws, uri));
    assert.match(permissions[0], /deny/);
    assert.match(permissions[1], /allow/);

    // the task's mark on its first changed line, and the agent's own mark on line 1
    const marks = await until('the task mark', () => {
      const found = vscode.languages.getDiagnostics(uri).filter(d => d.source === 'lackey' && d.code === 'task');
      return found.length > 0 ? found : undefined;
    });
    assert.equal(marks[0].range.start.line, 5);
    assert.ok(vscode.languages.getDiagnostics(uri).some(d => d.code === 'said' && d.range.start.line === 0 && d.message === 'lackey: total is a stub too'), why(ws, uri));

    // Show diff: VS Code's diff view, the text before the task on the left
    await press('lackey.keys.showDiff', doc, 5);
    const diff = await until('the diff', () => api.diffs()[0]);
    assert.equal(diff.before, STUB);
    const tab = await until('the diff tab', () => vscode.window.tabGroups.all.flatMap(group => group.tabs)
      .find(t => t.input instanceof vscode.TabInputTextDiff && t.input.original.scheme === 'lackey-before'));
    assert.equal((tab.input as vscode.TabInputTextDiff).modified.toString(), uri.toString());

    // Show journal opens the task's journal
    await press('lackey.keys.journal', doc, 5);
    await until('the journal', () => (vscode.window.activeTextEditor?.document.uri.fsPath.endsWith('journal.md') ? true : undefined));

    // Undo this task puts the buffer back, unsaved
    await press('lackey.keys.undoTask', doc, 5);
    await until('the buffer back at the stub', () => (doc.getText() === STUB ? true : undefined))
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}`); });
    // the undo edited the ghost's line, so the ghost is gone with it
    assert.deepEqual(api.ghosts(), []);
  });

  it('a running task is stopped with its key', async () => {
    writeFileSync(process.env.LACKEY_E2E_SCRIPT!, JSON.stringify({ steps: [{ sleep_ms: 60_000 }], cost: 0.01 }));
    const editor = await vscode.window.showTextDocument(doc, { viewColumn: vscode.ViewColumn.One });
    await editor.edit(edit => edit.insert(doc.lineAt(doc.lineCount - 1).range.end, '\n\ndef fee(cart):\n    pass\n'));
    const line = doc.lineCount - 3;
    assert.equal(doc.lineAt(line).text, 'def fee(cart):');
    await until('the offer on fee', () => actionStarting(uri, 'lackey: do this', line), 60_000)
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}`); });
    await press('lackey.keys.do', doc, line);
    await until('the stop action', () => actionStarting(uri, 'lackey: Stop this task', 0));
    await press('lackey.keys.stop', doc, 0);
    await until('the task stopped', () => (/"stopped"/.test(state(ws, 'tasks.jsonl')) ? true : undefined), 20_000)
      .catch(err => { throw new Error(`${String(err)}\n${why(ws, uri)}`); });
  });
});
