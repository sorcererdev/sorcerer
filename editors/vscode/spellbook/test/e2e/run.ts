import { execFileSync, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { runTests } from '@vscode/test-electron';

const ROOT = resolve(__dirname, '../../..');
const REPO = resolve(ROOT, '../../..');
const TARGET = join(REPO, 'target', 'debug');
const EXE = process.platform === 'win32' ? '.exe' : '';

// The end-to-end run: VS Code on spellbook's bistro corpus (conjure's bistro base with the
// corpus's generators and rules as .sorcerer/spellbook/, and the planted sqlite3 import), with
// the real `spellbook lsp`. SPELLBOOK_E2E_BINARY names the program when it is not the
// workspace's target/debug build. No model is called: spellbook calls none.
async function main(): Promise<void> {
  const spellbook = process.env.SPELLBOOK_E2E_BINARY ?? join(TARGET, `spellbook${EXE}`);
  if (!existsSync(spellbook)) throw new Error(`no spellbook at ${spellbook}: run cargo build -p sorcerer-spellbook, or set SPELLBOOK_E2E_BINARY`);
  // the native realpath: it spells a Windows short name (RUNNER~1) as the server will
  const base = realpathSync.native(mkdtempSync(join(tmpdir(), 'sb-e2e-')));
  try {
    await run(base, spellbook);
  } finally {
    rmSync(base, { recursive: true, force: true, maxRetries: 5 });
  }
}

function write(path: string, text: string): void {
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, text);
}

async function run(base: string, spellbook: string): Promise<void> {
  const ws = join(base, 'ws');
  makeProject(ws);
  // spellbook.path is read from user settings only
  write(join(base, 'ud', 'User', 'settings.json'), JSON.stringify({ 'spellbook.path': spellbook }));
  const dirs = userDirs(join(base, 'home'));
  warmUp(spellbook, ws, dirs);
  await runTests({
    version: process.env.VSCODE_VERSION ?? 'stable',
    extensionDevelopmentPath: ROOT,
    extensionTestsPath: join(__dirname, 'index.js'),
    // a name short enough for the socket VS Code puts in the user data directory
    launchArgs: [ws, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes',
      `--user-data-dir=${join(base, 'ud')}`],
    extensionTestsEnv: { SPELLBOOK_E2E_WS: ws, ...dirs },
  });
}

// makeProject copies the corpus into WS as one commit, then plants the check case (a command
// that imports sqlite3) as a second.
function makeProject(ws: string): void {
  cpSync(join(REPO, 'corpus', 'conjure', 'bistro', 'base'), ws, { recursive: true });
  cpSync(join(REPO, 'corpus', 'spellbook', 'bistro', 'spellbook'), join(ws, '.sorcerer', 'spellbook'), { recursive: true });
  write(join(ws, '.vscode', 'settings.json'), JSON.stringify({
    'security.workspace.trust.enabled': false,
    'workbench.localHistory.enabled': false,
  }, null, 2));
  const git = (...args: string[]) =>
    execFileSync('git', ['-c', 'user.name=e2e', '-c', 'user.email=e2e@example.com', '-c', 'core.autocrlf=false', ...args], { cwd: ws });
  git('init', '-q');
  git('add', '.');
  git('commit', '-q', '-m', 'start');
  git('apply', join(REPO, 'corpus', 'spellbook', 'bistro', 'cases', 'check', 'sqlite3-in-command.patch'));
  git('commit', '-q', '-a', '-m', 'a command imports sqlite3');
}

// userDirs is the user's config and cache directories (the trust list, the grammar cache) as
// the run's own, made.
function userDirs(home: string): Record<string, string> {
  const dirs: Record<string, string> = process.platform === 'win32'
    ? { APPDATA: join(home, 'AppData', 'Roaming'), LOCALAPPDATA: join(home, 'AppData', 'Local') }
    : { XDG_CONFIG_HOME: join(home, '.config'), XDG_CACHE_HOME: join(home, '.cache') };
  for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });
  return dirs;
}

// warmUp trusts the project (harmless when nothing needs it) and runs the rules once before
// VS Code starts: the grammar is built into the run's cache, and the planted case is proved
// (exit 1, an error finding) before the editor is involved.
function warmUp(spellbook: string, ws: string, dirs: Record<string, string>): void {
  const env = { ...process.env, ...dirs };
  execFileSync(spellbook, ['trust'], { cwd: ws, env });
  const check = spawnSync(spellbook, ['check', '--json'], { cwd: ws, env, encoding: 'utf8' });
  if (check.status !== 1) {
    throw new Error(`spellbook check should find the planted sqlite3 import (exit 1), got ${check.status}\n${check.stdout}\n${check.stderr}`);
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
