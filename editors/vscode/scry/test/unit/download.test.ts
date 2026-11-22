import * as assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { constants, accessSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { type AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { assetFor, assetUrl, binDir, install, releaseBase, releaseSize, sumOf, tarProgram, targetFor } from '../../src/download';

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

// The archive cargo-dist makes: one directory named for the archive, the binary inside it.
function archiveWith(text: string): Buffer {
  const dir = scratch();
  const inner = join(dir, `sorcerer-scry-${TARGET}`);
  mkdirSync(inner);
  writeFileSync(join(inner, EXE), text);
  execFileSync(tarProgram(), ['-czf', join(dir, ASSET), '-C', dir, `sorcerer-scry-${TARGET}`]);
  const out = readFileSync(join(dir, ASSET));
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
  // an older version's binary is cleared once the new one is in place
  mkdirSync(binDir(storage, '0.0.9'), { recursive: true });
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
