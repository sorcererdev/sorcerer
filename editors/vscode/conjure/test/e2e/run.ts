import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join, resolve } from 'node:path';
import { runTests } from '@vscode/test-electron';

const ROOT = resolve(__dirname, '../../..');
const TARGET = resolve(ROOT, '../../../target/debug');
const EXE = process.platform === 'win32' ? '.exe' : '';

// The end-to-end run: VS Code on a small project, with the real `conjure lsp` and a fake
// `claude` that answers every call with one canned change (see conjure.test.ts).
// CONJURE_E2E_BINARY and CONJURE_E2E_FAKE_CLAUDE name the programs when they are not the
// workspace's target/debug builds.
async function main(): Promise<void> {
  const conjure = process.env.CONJURE_E2E_BINARY ?? join(TARGET, `conjure${EXE}`);
  const fake = process.env.CONJURE_E2E_FAKE_CLAUDE ?? join(TARGET, `fake-claude${EXE}`);
  for (const [name, path] of [['conjure', conjure], ['fake-claude', fake]]) {
    if (!existsSync(path)) throw new Error(`no ${name} at ${path}: run cargo build -p sorcerer-conjure and cargo build -p sorcerer-models --bin fake-claude, or set CONJURE_E2E_*`);
  }
  // the native realpath: it spells a Windows short name (RUNNER~1) as the server will
  const base = realpathSync.native(mkdtempSync(join(tmpdir(), 'cj-e2e-')));
  try {
    await run(base, conjure, fake);
  } finally {
    rmSync(base, { recursive: true, force: true, maxRetries: 5 });
  }
}

function write(path: string, text: string): void {
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, text);
}

// The reply the fake claude gives: a goal, the changed file and a new test file.
function reply(): string {
  const text = '<goal>fill f</goal>'
    + '<file path="a.py">def f(x):\n    return x + 1\n</file>'
    + '<file path="a__test.py">from a import f\n\n\ndef test_f():\n    assert f(1) == 2\n</file>';
  return JSON.stringify({ result: text, total_cost_usd: 0.001, duration_ms: 1000, is_error: false, usage: { input_tokens: 3, output_tokens: 4 } });
}

async function run(base: string, conjure: string, fake: string): Promise<void> {
  const ws = join(base, 'ws');
  write(join(ws, 'a.py'), 'def f(x):\n    pass\n');
  write(join(ws, '.vscode/settings.json'), JSON.stringify({
    'security.workspace.trust.enabled': false,
    'workbench.localHistory.enabled': false,
  }, null, 2));
  const git = (...args: string[]) =>
    execFileSync('git', ['-c', 'user.name=e2e', '-c', 'user.email=e2e@example.com', ...args], { cwd: ws });
  git('init', '-q');
  git('add', '.');
  git('commit', '-q', '-m', 'start');
  // conjure picks the `claude` provider when a `claude` is on PATH: the fake, copied there. The
  // project's own config also runs the fake as the provider's runner, because VS Code on macOS
  // may put another PATH (the login shell's, with the real `claude`) before ours; a runner is why
  // the project must be trusted: `conjure trust`, below
  const bin = join(base, 'bin');
  mkdirSync(bin);
  copyFileSync(fake, join(bin, `claude${EXE}`));
  write(join(ws, '.sorcerer/config.toml'), `[model.providers.claude]\nrunner = '${fake}'\n`);
  // conjure.path is read from user settings only
  write(join(base, 'ud', 'User', 'settings.json'), JSON.stringify({ 'conjure.path': conjure }));
  // the user's config and cache directories (the trust list, the grammar cache) are the run's own
  const home = join(base, 'home');
  const dirs = process.platform === 'win32'
    ? { APPDATA: join(home, 'AppData', 'Roaming'), LOCALAPPDATA: join(home, 'AppData', 'Local') }
    : { XDG_CONFIG_HOME: join(home, '.config'), XDG_CACHE_HOME: join(home, '.cache') };
  for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });
  execFileSync(conjure, ['trust'], { cwd: ws, env: { ...process.env, ...dirs } });
  await runTests({
    version: process.env.VSCODE_VERSION ?? 'stable',
    extensionDevelopmentPath: ROOT,
    extensionTestsPath: join(__dirname, 'index.js'),
    // a name short enough for the socket VS Code puts in the user data directory
    launchArgs: [ws, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes',
      `--user-data-dir=${join(base, 'ud')}`],
    extensionTestsEnv: {
      CONJURE_E2E_WS: ws,
      FAKE_CLAUDE_REPLY: reply(),
      PATH: `${bin}${delimiter}${process.env.PATH ?? ''}`,
      ...dirs,
    },
  });
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
