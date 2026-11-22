import * as assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import * as vscode from 'vscode';

// The download suite (run.ts): no scry anywhere, the offer accepted, the binary checked and
// unpacked into the extension's storage, and the real `scry lsp` serving. The runner has no
// model provider and no Jev key, so a save is answered with scry's cannot-run message: proof
// the downloaded binary is the server, with no model called.

// What the extension said, by kind. The stubs go in when this file loads, before the
// extension starts (it starts once VS Code has finished starting up).
const said: { info: string[]; warning: string[]; error: string[] } = { info: [], warning: [], error: [] };
const window = vscode.window as unknown as Record<string, (message: string, ...rest: unknown[]) => Thenable<unknown>>;
window.showInformationMessage = async (message: string) => {
  said.info.push(message);
  return /is not installed\. Download it/.test(message) ? 'Download' : undefined;
};
window.showWarningMessage = async (message: string) => {
  said.warning.push(message);
  return undefined;
};
window.showErrorMessage = async (message: string) => {
  said.error.push(message);
  return undefined;
};

async function until<T>(what: string, ms: number, check: () => T | undefined): Promise<T> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const found = check();
    if (found !== undefined) return found;
    await new Promise(r => setTimeout(r, 250));
  }
  throw new Error(`${what} within ${ms / 1000} s; said: ${JSON.stringify(said)}`);
}

describe('scry (downloaded)', () => {
  it('offers the download, installs the checked binary, and the real server answers a save', async () => {
    // the extension starts once VS Code has finished starting up: start it now, stubs in place
    await vscode.extensions.getExtension('sorcererdev.scry')!.activate();
    const offer = await until('the offer', 60_000, () => said.info.find(m => /is not installed/.test(m)));
    assert.match(offer, /^scry \d+\.\d+\.\d+\S* is not installed\. Download it \(\d+ MB, from github\.com\/sorcererdev\/sorcerer releases\)\?$/);
    const binary = process.env.SCRY_E2E_BINARY_AT!;
    await until(`the binary at ${binary}`, 120_000, () => (existsSync(binary) ? true : undefined));
    const uri = vscode.Uri.file(join(process.env.SCRY_E2E_WS!, 'a.py'));
    const doc = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(doc);
    let lastSave = 0;
    const changeAndSave = async () => {
      lastSave = Date.now();
      await editor.edit(b => b.insert(new vscode.Position(1, doc.lineAt(1).text.length), ' * 2'));
      await doc.save();
    };
    // a save made before the server is up is not sent, and the first one the server gets
    // fetches and builds the grammar: change and save every few seconds until it answers
    const notReviewing = await until('scry\'s cannot-run message', 200_000, () => {
      const message = said.warning.find(m => m.startsWith('scry is not reviewing: '));
      if (message === undefined && Date.now() - lastSave > 5_000) void changeAndSave();
      return message;
    });
    assert.match(notReviewing, /^scry is not reviewing: .+\. Run "scry doctor"\.$/);
    assert.deepEqual(said.error, []);
  });
});
