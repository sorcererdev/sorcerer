import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { type AddressInfo } from 'node:net';
import { homedir, tmpdir } from 'node:os';
import { delimiter, join, resolve } from 'node:path';
import { runTests } from '@vscode/test-electron';
import { assetFor, tarProgram, targetFor } from '../../src/download';

const ROOT = resolve(__dirname, '../../..');
const EXE = process.platform === 'win32' ? 'scry.exe' : 'scry';

// The suites, each in a VS Code of its own (the extension finds its binary once, when it
// starts): `scry` against the fake server, `download` against the real scry served as a
// local release. SCRY_E2E_SUITES picks some (comma-separated); the default is both.
const SUITES: Record<string, (base: string) => Promise<void>> = { scry: fakeServerSuite, download: downloadSuite };

async function main(): Promise<void> {
  const chosen = (process.env.SCRY_E2E_SUITES ?? 'scry,download').split(',').map(s => s.trim()).filter(s => s !== '');
  for (const suite of chosen) {
    if (!SUITES[suite]) throw new Error(`no e2e suite ${suite}; the suites are ${Object.keys(SUITES).join(', ')}`);
    const base = realpathSync(mkdtempSync(join(tmpdir(), 'scry-e2e-')));
    try {
      await SUITES[suite](base);
    } finally {
      rmSync(base, { recursive: true, force: true, maxRetries: 5 });
    }
  }
}

function write(path: string, text: string): void {
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, text);
}

// runs VS Code on a workspace with the suite's test file, in a user data directory of its own
// (so no download or answer is remembered from an earlier run), named short: VS Code puts a
// socket in it, and macOS refuses a socket path over 103 characters
async function launch(suite: string, base: string, ws: string, env: Record<string, string>): Promise<void> {
  await runTests({
    version: process.env.VSCODE_VERSION ?? 'stable',
    extensionDevelopmentPath: ROOT,
    extensionTestsPath: join(__dirname, 'index.js'),
    launchArgs: [ws, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes',
      `--user-data-dir=${join(base, 'ud')}`],
    extensionTestsEnv: { SCRY_E2E_SUITE: suite, SCRY_E2E_WS: ws, ...env },
  });
}

// A project (a folder with .git) holding a.py, served by the fake `scry lsp`, which answers
// every save with one bug on line 2 of the file (the finding in replies/diagnostics.json).
async function fakeServerSuite(base: string): Promise<void> {
  const ws = join(base, 'ws');
  const replies = join(base, 'replies');
  mkdirSync(join(ws, '.git'), { recursive: true });
  write(join(ws, 'a.py'), 'def f(x):\n    return x + 2\n');
  write(join(replies, 'diagnostics.json'), JSON.stringify([
    { line: 1, code: 'bug', statement: 'f adds two, not one. The caller expects one.', rule: 'r1', id: 'f1' },
  ]));
  write(join(ws, '.vscode/settings.json'), JSON.stringify({
    'security.workspace.trust.enabled': false,
    'workbench.localHistory.enabled': false,
  }, null, 2));
  // scry.path is read from user settings only
  write(join(base, 'ud', 'User', 'settings.json'), JSON.stringify({ 'scry.path': join(ROOT, 'test/fake-lsp/fake-lsp') }));
  await launch('scry', base, ws, { FAKE_LSP_LOG: join(base, 'fake.log'), FAKE_LSP_REPLIES: replies });
}

// A git project with a.py committed, no scry anywhere the extension looks, no model, and this
// extension's version of scry (SCRY_E2E_BINARY, else the workspace's target/debug build)
// packed as the release packs it and served from 127.0.0.1 through SCRY_RELEASES_URL.
async function downloadSuite(base: string): Promise<void> {
  const own = ownScry();
  if (own && !process.env.CI) {
    console.log(`the download suite is skipped: the extension would find your own ${own} (CI runs it)`);
    return;
  }
  if (own) throw new Error(`the download suite needs a machine with no scry, and ${own} exists`);
  const version = String(JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')).version);
  const ws = join(base, 'ws');
  write(join(ws, 'a.py'), 'def total(prices):\n    return sum(prices)\n');
  const git = (...args: string[]) =>
    execFileSync('git', ['-c', 'user.name=e2e', '-c', 'user.email=e2e@example.com', ...args], { cwd: ws });
  git('init', '-q');
  git('add', '.');
  git('commit', '-q', '-m', 'start');
  // a workspace's scry.path is ignored: were it read, the extension would run node and never
  // offer the download
  write(join(ws, '.vscode/settings.json'), JSON.stringify({
    'scry.path': process.execPath,
    'security.workspace.trust.enabled': false,
    'workbench.localHistory.enabled': false,
  }, null, 2));
  const server = await serveDirectory(packRelease(join(base, 'release'), version));
  try {
    await launch('download', base, ws, {
      SCRY_RELEASES_URL: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
      SCRY_E2E_BINARY_AT: join(base, 'ud', 'User', 'globalStorage', 'sorcererdev.scry', 'bin', version, EXE),
      ...withoutModel(),
    });
  } finally {
    server.close();
  }
}

// packRelease lays out <dir>/v<version>/<asset> and <asset>.sha256 as cargo-dist does
// (checked against v0.1.0-rc.1): a .zip holds the files at its top, a .tar.gz holds one
// directory named for the archive; the sum is `<sum> *<asset>`.
function packRelease(dir: string, version: string): string {
  const binary = process.env.SCRY_E2E_BINARY ?? resolve(ROOT, '../../../target/debug', EXE);
  if (!existsSync(binary)) throw new Error(`no scry at ${binary}: run cargo build -p sorcerer-scry, or set SCRY_E2E_BINARY`);
  const target = targetFor(process.platform, process.arch);
  if (!target) throw new Error(`the release has no target for ${process.platform} ${process.arch}`);
  const asset = assetFor('scry', target);
  const stem = asset.replace(/\.(zip|tar\.gz)$/, '');
  const stage = join(dir, 'stage');
  const top = asset.endsWith('.zip') ? stage : join(stage, stem);
  mkdirSync(top, { recursive: true });
  writeFileSync(join(top, EXE), readFileSync(binary), { mode: 0o755 });
  writeFileSync(join(top, 'README.md'), 'scry\n');
  const out = join(dir, `v${version}`);
  mkdirSync(out, { recursive: true });
  // -a picks the format from the name: .zip or .tar.gz
  execFileSync(tarProgram(), ['-a', '-cf', join(out, asset), '-C', stage, ...readdirSync(stage)]);
  const sum = createHash('sha256').update(readFileSync(join(out, asset))).digest('hex');
  writeFileSync(join(out, `${asset}.sha256`), `${sum} *${asset}\n`);
  return dir;
}

// serveDirectory serves a directory's files over http on a free local port
async function serveDirectory(dir: string): Promise<Server> {
  const server = createServer((req, res) => {
    const file = join(dir, decodeURIComponent((req.url ?? '/').split('?')[0]));
    if (!file.startsWith(dir) || !existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    const body = readFileSync(file);
    res.writeHead(200, { 'content-length': body.length }).end(body);
  });
  await new Promise<void>(res => server.listen(0, '127.0.0.1', res));
  return server;
}

// withoutModel: no model can be called (the keys emptied, which scry reads as unset; without
// Jev scry does not review) and, on Unix, a PATH without a directory holding scry or claude.
// The Windows runner has neither, and its PATH is left alone.
function withoutModel(): Record<string, string> {
  const keys = { JEV_API_KEY: '', ANTHROPIC_API_KEY: '' };
  if (process.platform === 'win32') return keys;
  const path = (process.env.PATH ?? '').split(delimiter)
    .filter(dir => !existsSync(join(dir, EXE)) && !existsSync(join(dir, 'claude')))
    .join(delimiter);
  return { ...keys, PATH: path };
}

// ownScry is a scry in the home directories the extension looks in. VS Code hangs with a home
// of its own, so a developer who has one cannot run the download suite; CI never has one.
function ownScry(): string | undefined {
  return [join(homedir(), '.cargo', 'bin', EXE), join(homedir(), '.local', 'bin', EXE)].find(p => existsSync(p));
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
