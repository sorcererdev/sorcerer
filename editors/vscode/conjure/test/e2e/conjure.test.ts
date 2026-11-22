import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
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

// the code action titled TITLE on LINE (from 0) of URI
async function actionTitled(uri: vscode.Uri, title: string, line = 0): Promise<vscode.CodeAction | undefined> {
  // a request made before the server is up is cancelled: ask again
  const actions = await Promise.resolve(vscode.commands.executeCommand<vscode.CodeAction[]>(
    'vscode.executeCodeActionProvider', uri, new vscode.Range(line, 0, line, 0))).catch(() => undefined);
  return (actions ?? []).find(a => a.title === title);
}

async function run(action: vscode.CodeAction): Promise<void> {
  assert.ok(action.command, `${action.title} is a command`);
  await vscode.commands.executeCommand(action.command.command, ...(action.command.arguments ?? []));
}

// whatConjureKept is the project's change records and the Problems the run ended with, for a
// failure message: what the server decided when the change did not land.
function whatConjureKept(ws: string): string {
  try {
    return readFileSync(join(ws, '.sorcerer', 'state', 'conjure', 'changes.jsonl'), 'utf8');
  } catch {
    return 'no change was recorded';
  }
}

// The fake-claude suite: run by the ubuntu and windows CI jobs and by hand (npm run test:e2e).
describe('conjure (fake claude)', () => {
  it('Conjure here lands in the buffers, marks the changed line, and Undo puts it back, within 20 s', async () => {
    // the messages the server says (busy, not trusted, …), for a failure message
    const said: string[] = [];
    for (const name of ['showInformationMessage', 'showWarningMessage', 'showErrorMessage'] as const) {
      (vscode.window as unknown as Record<string, unknown>)[name] = (text: string) => {
        said.push(text);
        return Promise.resolve(undefined);
      };
    }
    await vscode.extensions.getExtension('sorcererdev.conjure')!.activate();
    const ws = process.env.CONJURE_E2E_WS!;
    const uri = vscode.Uri.file(join(ws, 'a.py'));
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);

    // the server answers the code actions once it is up
    const conjure = await until('the Conjure here action', () => actionTitled(uri, 'Conjure here'));
    await run(conjure);

    await until('the new text in the buffer', () => (doc.getText().includes('return x + 1') ? true : undefined), 60_000)
      .catch(err => { throw new Error(`${String(err)}\n${whatConjureKept(ws)}\nmarks: ${JSON.stringify(vscode.languages.getDiagnostics(uri).map(d => d.message))}\nsaid: ${JSON.stringify(said)}`); });
    assert.equal(readFileSync(join(ws, 'a.py'), 'utf8'), 'def f(x):\n    pass\n', 'nothing was saved: the disk still says pass');
    // compared as URIs: VS Code writes a Windows drive letter in lower case
    const testUri = vscode.Uri.file(join(ws, 'a__test.py')).toString();
    const testDoc = await until('the test file open',
      () => vscode.workspace.textDocuments.find(d => d.uri.toString() === testUri))
      .catch(err => { throw new Error(`${String(err)}\nexpected ${testUri}\nopen: ${vscode.workspace.textDocuments.map(d => d.uri.toString()).join(' ')}`); });
    assert.match(testDoc.getText(), /def test_f\(\):/);
    const marks = await until('the mark',
      () => {
        const found = vscode.languages.getDiagnostics(uri).filter(d => d.source === 'conjure');
        return found.length > 0 ? found : undefined;
      });
    // the mark sits on the first changed line of the file: `pass` became `return x + 1`
    assert.equal(marks[0].range.start.line, 1);
    assert.match(marks[0].message, /^fill f · /);

    const undo = await until('the Undo action', () => actionTitled(uri, 'conjure: Undo', 1))
      .catch(err => { throw new Error(`${String(err)}\nmarks: ${JSON.stringify(vscode.languages.getDiagnostics().flatMap(([, ds]) => ds.map(d => d.message)))}\nsaid: ${JSON.stringify(said)}\n${whatConjureKept(ws)}`); });
    await run(undo);
    await until('the buffer back at pass', () => (doc.getText() === 'def f(x):\n    pass\n' ? true : undefined));
  });
});
