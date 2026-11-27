import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { runTests } from '@vscode/test-electron';

const ROOT = resolve(__dirname, '../../..');
const TARGET = resolve(ROOT, '../../../target/debug');
const EXE = process.platform === 'win32' ? '.exe' : '';

// The end-to-end run: VS Code on a small project, with the real `apprentice lsp` and the fake
// `claude` its harness tests run (`apprentice-fake-claude`), which plays the script the test
// writes to APPRENTICE_E2E_SCRIPT and really calls apprentice's tools and hooks (see
// apprentice.test.ts). APPRENTICE_E2E_BINARY and APPRENTICE_E2E_FAKE_CLAUDE name the programs
// when they are not the workspace's target/debug builds.
async function main(): Promise<void> {
  const apprentice = process.env.APPRENTICE_E2E_BINARY ?? join(TARGET, `apprentice${EXE}`);
  const fake = process.env.APPRENTICE_E2E_FAKE_CLAUDE ?? join(TARGET, `apprentice-fake-claude${EXE}`);
  for (const [name, path] of [['apprentice', apprentice], ['apprentice-fake-claude', fake]]) {
    if (!existsSync(path)) throw new Error(`no ${name} at ${path}: run cargo build -p sorcerer-apprentice, or set APPRENTICE_E2E_*`);
  }
  // the native realpath: it spells a Windows short name (RUNNER~1) as the server will
  const base = realpathSync.native(mkdtempSync(join(tmpdir(), 'ap-e2e-')));
  try {
    await run(base, apprentice, fake);
  } finally {
    rmSync(base, { recursive: true, force: true, maxRetries: 5 });
  }
}

function write(path: string, text: string): void {
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, text);
}

// toml is a TOML string of a path: single quotes, so a Windows backslash is no escape.
function toml(path: string): string {
  return `'${path}'`;
}

async function run(base: string, apprentice: string, fake: string): Promise<void> {
  const ws = join(base, 'ws');
  write(join(ws, 'cart.py'), 'def total(cart):\n    return 1\n');
  write(join(ws, '.vscode/settings.json'), JSON.stringify({
    'security.workspace.trust.enabled': false,
    'workbench.localHistory.enabled': false,
  }, null, 2));
  const git = (...args: string[]) =>
    execFileSync('git', ['-c', 'user.name=e2e', '-c', 'user.email=e2e@example.com', ...args], { cwd: ws });
  git('init', '-q');
  git('add', '.');
  git('commit', '-q', '-m', 'start');
  // the project's config runs the fake as `claude` (a runner is why the project must be
  // trusted: `apprentice trust`, below); no JEV_API_KEY, so only an empty body is offered
  write(join(ws, '.sorcerer/config.toml'), `[model.providers.claude]\nrunner = ${toml(fake)}\n`);
  // apprentice.path is read from user settings only
  write(join(base, 'ud', 'User', 'settings.json'), JSON.stringify({ 'apprentice.path': apprentice }));
  // the user's config and cache directories (the trust list, the grammar cache) are the run's own
  const home = join(base, 'home');
  const dirs = process.platform === 'win32'
    ? { APPDATA: join(home, 'AppData', 'Roaming'), LOCALAPPDATA: join(home, 'AppData', 'Local') }
    : { XDG_CONFIG_HOME: join(home, '.config'), XDG_CACHE_HOME: join(home, '.cache') };
  for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });
  // VS Code passes this process's environment on: no Jev key reaches the server
  delete process.env.JEV_API_KEY;
  execFileSync(apprentice, ['trust'], { cwd: ws, env: { ...process.env, ...dirs } });
  await runTests({
    version: process.env.VSCODE_VERSION ?? 'stable',
    extensionDevelopmentPath: ROOT,
    extensionTestsPath: join(__dirname, 'index.js'),
    // a name short enough for the socket VS Code puts in the user data directory
    launchArgs: [ws, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes',
      `--user-data-dir=${join(base, 'ud')}`],
    extensionTestsEnv: {
      APPRENTICE_E2E_WS: ws,
      APPRENTICE_E2E_SCRIPT: join(base, 'script.json'),
      FAKE_APPRENTICE_SCRIPT: join(base, 'script.json'),
      FAKE_APPRENTICE_LOG: join(base, 'fake.jsonl'),
      ...dirs,
    },
  });
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
