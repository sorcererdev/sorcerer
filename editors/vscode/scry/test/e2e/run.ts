import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { runTests } from '@vscode/test-electron';

const ROOT = resolve(__dirname, '../../..');

function write(path: string, text: string): void {
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, text);
}

// A project (a folder with .git) holding a.py, served by the fake `scry lsp`, which answers
// every save with one bug on line 2 of the file (the finding in replies/diagnostics.json).
async function main(): Promise<void> {
  const base = realpathSync(mkdtempSync(join(tmpdir(), 'scry-e2e-')));
  const ws = join(base, 'ws');
  const replies = join(base, 'replies');
  try {
    mkdirSync(join(ws, '.git'), { recursive: true });
    write(join(ws, 'a.py'), 'def f(x):\n    return x + 2\n');
    write(join(replies, 'diagnostics.json'), JSON.stringify([
      { line: 1, code: 'bug', statement: 'f adds two, not one. The caller expects one.', rule: 'r1', id: 'f1' },
    ]));
    write(join(ws, '.vscode/settings.json'), JSON.stringify({
      'scry.path': join(ROOT, 'test/fake-lsp/fake-lsp'),
      'security.workspace.trust.enabled': false,
      'workbench.localHistory.enabled': false,
    }, null, 2));
    await runTests({
      version: process.env.VSCODE_VERSION ?? 'stable',
      extensionDevelopmentPath: ROOT,
      extensionTestsPath: join(__dirname, 'index.js'),
      launchArgs: [ws, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes'],
      extensionTestsEnv: { SCRY_E2E_WS: ws, FAKE_LSP_LOG: join(base, 'fake.log'), FAKE_LSP_REPLIES: replies },
    });
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
