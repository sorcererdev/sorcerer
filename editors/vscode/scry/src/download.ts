import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { chmod, mkdir, mkdtemp, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

// The download of scry's own release: which archive fits this machine, fetching it and its
// sum, and unpacking the checked binary into the extension's storage. Nothing here asks the
// user; extension.ts does that.

const RELEASES = 'https://github.com/sorcererdev/sorcerer/releases/download';

// targetFor is the release's name for this machine (Node's `process.platform` and
// `process.arch`), or undefined when the release has no binary for it.
//
// Example: targetFor('darwin', 'arm64') is 'aarch64-apple-darwin'; targetFor('linux', 'arm')
// is undefined.
export function targetFor(platform: NodeJS.Platform, arch: string): string | undefined {
  const targets: Record<string, string> = {
    'darwin arm64': 'aarch64-apple-darwin',
    'darwin x64': 'x86_64-apple-darwin',
    'linux x64': 'x86_64-unknown-linux-gnu',
    'linux arm64': 'aarch64-unknown-linux-gnu',
    'win32 x64': 'x86_64-pc-windows-msvc',
  };
  return targets[`${platform} ${arch}`];
}

// assetFor is the release file holding scry for a target: a .zip for Windows, a .tar.gz for
// the others. Its sum is the same name with .sha256 added.
export function assetFor(target: string): string {
  return `sorcerer-scry-${target}${target.endsWith('-windows-msvc') ? '.zip' : '.tar.gz'}`;
}

// releaseBase is where the releases are downloaded from: GitHub, or SCRY_RELEASES_URL when it
// is set (the end-to-end test serves a local release there).
export function releaseBase(env: NodeJS.ProcessEnv): string {
  return (env.SCRY_RELEASES_URL ?? RELEASES).replace(/\/+$/, '');
}

// assetUrl is the address of a version's archive for a target.
//
// Example: assetUrl(base, '0.1.0', 'aarch64-apple-darwin') is
// `${base}/v0.1.0/sorcerer-scry-aarch64-apple-darwin.tar.gz`.
export function assetUrl(base: string, version: string, target: string): string {
  return `${base}/v${version}/${assetFor(target)}`;
}

// binDir is the directory a version's downloaded binary lives in, inside the extension's
// storage: one directory per version, so a new extension version never runs the old binary.
export function binDir(storage: string, version: string): string {
  return join(storage, 'bin', version);
}

// sumOf reads the SHA-256 out of a `.sha256` file as cargo-dist writes it (`<64 hex digits>
// <file name>`), in lower case, or undefined when there is none (say an HTML error page).
export function sumOf(text: string): string | undefined {
  return /\b[0-9a-fA-F]{64}\b/.exec(text)?.[0].toLowerCase();
}

// tarProgram is the tar to unpack with. On Windows it is the one Windows ships (it reads .zip
// too), not a GNU tar from Git that may come first on PATH and reads neither .zip nor `C:`.
export function tarProgram(): string {
  return process.platform === 'win32' ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
}

// install downloads a version's archive for a target, checks it against its `.sha256`, and
// unpacks the binary into binDir(storage, version), returning the binary's path:
// 1. fetch the archive and its sum; a file that is not there is an error naming it;
// 2. compare the sums before anything is written: a mismatch installs nothing;
// 3. unpack in a scratch directory under storage, move the binary into place, make it
//    executable, and clear the scratch directory whatever happened;
// 4. remove other versions' binaries, which this extension version never runs.
export async function install(base: string, version: string, target: string, storage: string): Promise<string> {
  const url = assetUrl(base, version, target);
  const [archive, sumText] = await Promise.all([fetched(url), fetched(`${url}.sha256`)]);
  if (sumOf(sumText.toString('utf8')) !== createHash('sha256').update(archive).digest('hex')) {
    throw new Error("the download's checksum did not match; nothing was installed");
  }
  await mkdir(join(storage, 'bin'), { recursive: true });
  const scratch = await mkdtemp(join(storage, 'bin', '.download-'));
  try {
    const binary = await unpacked(archive, assetFor(target), scratch, binDir(storage, version));
    await removeOtherVersions(storage, version);
    return binary;
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

// releaseSize asks for a version's archive without downloading it: its size in bytes (0 when
// the server does not say), or undefined when the release has no such file (no release for
// this version, say). A network failure is an error.
export async function releaseSize(url: string): Promise<number | undefined> {
  const response = await fetch(url, { method: 'HEAD', signal: AbortSignal.timeout(30_000) });
  if (!response.ok) return undefined;
  return Number(response.headers.get('content-length') ?? 0);
}

// fetched is the body of a URL, or an error naming the URL and the HTTP status.
async function fetched(url: string): Promise<Buffer> {
  const response = await fetch(url, { signal: AbortSignal.timeout(300_000) });
  if (!response.ok) throw new Error(`could not download ${url}: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

// unpacked writes a checked archive into scratch, unpacks it there, and moves the binary to
// dest; the archive holds it at its top or in one directory named for the archive.
async function unpacked(archive: Buffer, asset: string, scratch: string, dest: string): Promise<string> {
  const name = process.platform === 'win32' ? 'scry.exe' : 'scry';
  const file = join(scratch, asset);
  const into = join(scratch, 'unpacked');
  await writeFile(file, archive);
  await mkdir(into);
  await promisify(execFile)(tarProgram(), ['-xf', file, '-C', into]);
  const root = asset.replace(/\.(zip|tar\.gz)$/, '');
  const found = [join(into, name), join(into, root, name)].find(p => existsSync(p));
  if (!found) throw new Error(`${asset} holds no ${name}; nothing was installed`);
  if (process.platform !== 'win32') await chmod(found, 0o755);
  await mkdir(dest, { recursive: true });
  await rename(found, join(dest, name));
  return join(dest, name);
}

// removeOtherVersions deletes the binaries other extension versions downloaded; one that is
// in use (another window still runs it) stays until next time.
async function removeOtherVersions(storage: string, version: string): Promise<void> {
  for (const entry of await readdir(join(storage, 'bin'))) {
    if (entry !== version && !entry.startsWith('.')) {
      await rm(join(storage, 'bin', entry), { recursive: true, force: true }).catch(() => undefined);
    }
  }
}
