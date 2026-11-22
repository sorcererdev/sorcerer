import * as assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { constants, accessSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, utimesSync, writeFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { type AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { assetFor, assetUrl, binDir, bodyUpTo, install, releaseBase, releaseSize, sumOf, tarProgram, targetFor } from '../../src/download';

const EXE = process.platform === 'win32' ? 'scry.exe' : 'scry';
const TARGET = 'x86_64-unknown-linux-gnu';
const ASSET = `sorcerer-scry-${TARGET}.tar.gz`;

test('targets_for_the_five_platforms', () => {
  assert.equal(targetFor('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(targetFor('darwin', 'x64'), 'x86_64-apple-darwin');
  assert.equal(targetFor('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.equal(targetFor('linux', 'arm64'), 'aarch64-unknown-linux-gnu');
  assert.equal(targetFor('win32', 'x64'), 'x86_64-pc-windows-msvc');
});

test('no_target_for_an_unsupported_platform', () => {
  assert.equal(targetFor('linux', 'arm'), undefined);
  assert.equal(targetFor('win32', 'arm64'), undefined);
  assert.equal(targetFor('freebsd', 'x64'), undefined);
});

test('assets_are_zip_on_windows_and_tar_gz_elsewhere', () => {
  assert.equal(assetFor('x86_64-pc-windows-msvc'), 'sorcerer-scry-x86_64-pc-windows-msvc.zip');
  assert.equal(assetFor('aarch64-apple-darwin'), 'sorcerer-scry-aarch64-apple-darwin.tar.gz');
});

test('the release base is github unless SCRY_RELEASES_URL says otherwise', () => {
  assert.equal(releaseBase({}), 'https://github.com/sorcererdev/sorcerer/releases/download');
  assert.equal(releaseBase({ SCRY_RELEASES_URL: 'http://127.0.0.1:9/r/' }), 'http://127.0.0.1:9/r');
});

test('sum_reads_cargo_dists_format', () => {
  const hex = 'AB'.repeat(32);
  assert.equal(sumOf(`${hex}  ${ASSET}\n`), hex.toLowerCase());
  assert.equal(sumOf(`${hex} *${ASSET}`), hex.toLowerCase());
  assert.equal(sumOf('<html>Not Found</html>'), undefined);
  assert.equal(sumOf('ab12'), undefined);
});

// A release served from memory: path -> body.
const servers: Server[] = [];
after(() => servers.forEach(s => s.close()));

async function serve(files: Record<string, Buffer | string>): Promise<string> {
  const server = createServer((req, res) => {
    const body = files[req.url ?? ''];
    if (body === undefined) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-length': Buffer.byteLength(body) }).end(body);
  });
  servers.push(server);
  await new Promise<void>(res => server.listen(0, '127.0.0.1', res));
  return `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
}

function scratch(): string {
  return mkdtempSync(join(tmpdir(), 'scry-download-'));
}

// An archive laid out as cargo-dist lays out the release's (checked against v0.1.0-rc.1): a
// .tar.gz holds one directory named for the archive with the files in it; a .zip holds the
// files at its top. `extra` adds files beside the binary; `link` makes the binary a symbolic
// link to a file beside it instead.
function archiveWith(text: string, asset = ASSET, extra: Record<string, string> = {}, link = false): Buffer {
  const dir = scratch();
  const stem = asset.replace(/\.(zip|tar\.gz)$/, '');
  const top = asset.endsWith('.zip') ? join(dir, 'files') : join(dir, 'files', stem);
  mkdirSync(top, { recursive: true });
  for (const [name, body] of Object.entries({ 'README.md': 'read me', ...extra })) writeFileSync(join(top, name), body);
  if (link) {
    writeFileSync(join(top, 'real'), text);
    symlinkSync('real', join(top, EXE));
  } else {
    writeFileSync(join(top, EXE), text);
  }
  execFileSync(tarProgram(), ['-a', '-cf', join(dir, asset), '-C', join(dir, 'files'), ...readdirSync(join(dir, 'files'))]);
  const out = readFileSync(join(dir, asset));
  rmSync(dir, { recursive: true, force: true });
  return out;
}

function sha256(b: Buffer): string {
  return createHash('sha256').update(b).digest('hex');
}

test('a_mismatched_sum_deletes_and_says_so', async () => {
  const archive = archiveWith('#!/bin/sh\necho hi\n');
  const base = await serve({ [`/v0.1.0/${ASSET}`]: archive, [`/v0.1.0/${ASSET}.sha256`]: `${'0'.repeat(64)}  ${ASSET}\n` });
  const storage = scratch();
  await assert.rejects(install(base, '0.1.0', TARGET, storage),
    { message: "the download's checksum did not match; nothing was installed" });
  assert.ok(!existsSync(join(binDir(storage, '0.1.0'), EXE)), 'no binary');
  const left = existsSync(join(storage, 'bin')) ? readdirSync(join(storage, 'bin')) : [];
  assert.deepEqual(left, [], 'nothing left behind');
  rmSync(storage, { recursive: true, force: true });
});

test('a missing release says which file was not found', async () => {
  const base = await serve({});
  const storage = scratch();
  await assert.rejects(install(base, '9.9.9', TARGET, storage), /HTTP 404/);
  rmSync(storage, { recursive: true, force: true });
});

test('a_matching_download_unpacks_and_is_executable', async () => {
  const archive = archiveWith('#!/bin/sh\necho hi\n');
  const storage = scratch();
  // an older version's binary, and a download a killed VS Code left an hour ago, are cleared
  mkdirSync(binDir(storage, '0.0.9'), { recursive: true });
  mkdirSync(join(storage, 'bin', '.download-old'));
  const twoHoursAgo = new Date(Date.now() - 2 * 3600_000);
  utimesSync(join(storage, 'bin', '.download-old'), twoHoursAgo, twoHoursAgo);
  const base = await serve({ [`/v0.1.0/${ASSET}`]: archive, [`/v0.1.0/${ASSET}.sha256`]: `${sha256(archive)}  ${ASSET}\n` });
  const binary = await install(base, '0.1.0', TARGET, storage);
  assert.equal(binary, join(binDir(storage, '0.1.0'), EXE));
  assert.equal(readFileSync(binary, 'utf8'), '#!/bin/sh\necho hi\n');
  if (process.platform !== 'win32') accessSync(binary, constants.X_OK);
  assert.deepEqual(readdirSync(join(storage, 'bin')), ['0.1.0']);
  rmSync(storage, { recursive: true, force: true });
});

test('the size is asked for without a download; a missing release has none', async () => {
  const base = await serve({ [`/v0.1.0/${ASSET}`]: Buffer.alloc(12_345_678) });
  assert.equal(await releaseSize(assetUrl(base, '0.1.0', TARGET)), 12_345_678);
  assert.equal(await releaseSize(assetUrl(base, '9.9.9', TARGET)), undefined);
});

test('the size check fails, not "no release", on a server error', async () => {
  const server = createServer((_req, res) => res.writeHead(503).end());
  servers.push(server);
  await new Promise<void>(res => server.listen(0, '127.0.0.1', res));
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  await assert.rejects(releaseSize(assetUrl(base, '0.1.0', TARGET)), /HTTP 503/);
});

async function installed(asset: string, target: string, archive: Buffer): Promise<{ storage: string; result: Promise<string> }> {
  const storage = scratch();
  const base = await serve({ [`/v0.1.0/${asset}`]: archive, [`/v0.1.0/${asset}.sha256`]: `${sha256(archive)} *${asset}\n` });
  return { storage, result: install(base, '0.1.0', target, storage) };
}

// GNU tar (Linux) writes no .zip; the Windows CI job unpacks a real one end to end
const writesZip = process.platform !== 'linux';

test('a_zip_with_its_files_at_the_top_unpacks', { skip: !writesZip }, async () => {
  const target = 'x86_64-pc-windows-msvc';
  const { storage, result } = await installed(assetFor(target), target, archiveWith('zip body', assetFor(target)));
  assert.equal(readFileSync(await result, 'utf8'), 'zip body');
  rmSync(storage, { recursive: true, force: true });
});

test('a_binary_that_is_a_link_is_refused_and_nothing_is_left', { skip: process.platform === 'win32' }, async () => {
  const { storage, result } = await installed(ASSET, TARGET, archiveWith('linked', ASSET, {}, true));
  await assert.rejects(result, /holds a link; nothing was installed/);
  assert.deepEqual(readdirSync(join(storage, 'bin')), []);
  rmSync(storage, { recursive: true, force: true });
});

test('an_archive_with_any_link_is_refused', { skip: process.platform === 'win32' }, async () => {
  const dir = scratch();
  const stem = `sorcerer-scry-${TARGET}`;
  mkdirSync(join(dir, stem));
  writeFileSync(join(dir, stem, EXE), 'body');
  symlinkSync('/etc', join(dir, stem, 'etc'));
  execFileSync(tarProgram(), ['-czf', join(dir, ASSET), '-C', dir, stem]);
  const { storage, result } = await installed(ASSET, TARGET, readFileSync(join(dir, ASSET)));
  await assert.rejects(result, /holds a link; nothing was installed/);
  assert.ok(!lstatSync(join(storage, 'bin')).isSymbolicLink());
  assert.deepEqual(readdirSync(join(storage, 'bin')), []);
  rmSync(storage, { recursive: true, force: true });
  rmSync(dir, { recursive: true, force: true });
});

test('a body past the cap is refused', async () => {
  await assert.rejects(bodyUpTo(new Response('x'.repeat(10)), 5, 'the archive'), /the archive is larger than 5 bytes/);
  assert.equal((await bodyUpTo(new Response('x'.repeat(5)), 5, 'the archive')).length, 5);
});
