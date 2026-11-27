import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmod, lstat, mkdir, mkdtemp, readdir, rename, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

// The download of a product's (scry's, conjure's) own release: which archive fits this machine, fetching it and its
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

// assetFor is the release file holding a product for a target: a .zip for Windows, a .tar.gz
// for the others. Its sum is the same name with .sha256 added.
//
// Example: assetFor('conjure', 'aarch64-apple-darwin') is
// 'sorcerer-conjure-aarch64-apple-darwin.tar.gz'.
export function assetFor(product: string, target: string): string {
  return `sorcerer-${product}-${target}${target.endsWith('-windows-msvc') ? '.zip' : '.tar.gz'}`;
}

// releaseBase is where the releases are downloaded from: GitHub, or the product's variable
// (SCRY_RELEASES_URL, CONJURE_RELEASES_URL) when it is set (the end-to-end test serves a local
// release there).
export function releaseBase(product: string, env: NodeJS.ProcessEnv): string {
  return (env[`${product.toUpperCase()}_RELEASES_URL`] ?? RELEASES).replace(/\/+$/, '');
}

// assetUrl is the address of a version's archive for a target.
//
// Example: assetUrl(base, 'scry', '0.1.0', 'aarch64-apple-darwin') is
// `${base}/v0.1.0/sorcerer-scry-aarch64-apple-darwin.tar.gz`.
export function assetUrl(base: string, product: string, version: string, target: string): string {
  return `${base}/v${version}/${assetFor(product, target)}`;
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

// The largest archive accepted: a product's is about 12 MB.
const MAX_ARCHIVE_BYTES = 200_000_000;
// How long a scratch directory may sit before it is taken for one a killed VS Code left.
const STALE_SCRATCH_MS = 3_600_000;

// install downloads a version's archive for a target, checks it against its `.sha256`, and
// unpacks the binary into binDir(storage, version), returning the binary's path:
// 1. fetch the archive (at most 200 MB) and its sum; a file that is not there is an error
//    naming it;
// 2. compare the sums before anything is written: a mismatch installs nothing;
// 3. unpack in a scratch directory under storage, refuse a tree holding any link, move the
//    binary into place, make it executable, and clear the scratch directory whatever happened;
// 4. remove other versions' binaries, which this extension version never runs, and scratch
//    directories a killed VS Code left.
export async function install(base: string, product: string, version: string, target: string, storage: string): Promise<string> {
  const url = assetUrl(base, product, version, target);
  const [archive, sumText] = await Promise.all([fetched(url, MAX_ARCHIVE_BYTES), fetched(`${url}.sha256`, 4096)]);
  if (sumOf(sumText.toString('utf8')) !== createHash('sha256').update(archive).digest('hex')) {
    throw new Error("the download's checksum did not match; nothing was installed");
  }
  await mkdir(join(storage, 'bin'), { recursive: true });
  const scratch = await mkdtemp(join(storage, 'bin', '.download-'));
  try {
    const binary = await unpacked(archive, assetFor(product, target), scratch, binDir(storage, version), exeName(product));
    await removeLeftovers(storage, version);
    return binary;
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

// releaseSize asks for a version's archive without downloading it: its size in bytes (0 when
// the server does not say), or undefined when the release has no such file (HTTP 404 or 410:
// no release for this version, say). Any other answer, and a network failure, is an error.
export async function releaseSize(url: string): Promise<number | undefined> {
  const response = await fetch(url, { method: 'HEAD', signal: AbortSignal.timeout(30_000) });
  if (response.status === 404 || response.status === 410) return undefined;
  if (!response.ok) throw new Error(`HTTP ${response.status} from ${url}`);
  return Number(response.headers.get('content-length') ?? 0);
}

// bodyUpTo reads a response's body, refusing one longer than max bytes as it arrives, so a
// misbehaving server cannot fill the memory.
//
// Example: bodyUpTo(response, 5, 'the archive') of a 10-byte body fails with "the archive is
// larger than 5 bytes".
export async function bodyUpTo(response: Response, max: number, what: string): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let length = 0;
  for await (const chunk of response.body ?? []) {
    length += chunk.length;
    if (length > max) throw new Error(`${what} is larger than ${max} bytes`);
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks);
}

// fetched is the body of a URL (at most max bytes), or an error naming the URL and the HTTP
// status.
async function fetched(url: string, max: number): Promise<Buffer> {
  const response = await fetch(url, { signal: AbortSignal.timeout(300_000) });
  if (!response.ok) throw new Error(`could not download ${url}: HTTP ${response.status}`);
  return bodyUpTo(response, max, url);
}

// unpacked writes a checked archive into scratch, unpacks it there, and moves the binary to
// dest.
async function unpacked(archive: Buffer, asset: string, scratch: string, dest: string, exe: string): Promise<string> {
  const file = join(scratch, asset);
  const into = join(scratch, 'unpacked');
  await writeFile(file, archive);
  await mkdir(into);
  await promisify(execFile)(tarProgram(), ['-xf', file, '-C', into]);
  await refuseLinks(into, asset);
  return movedInto(await binaryIn(into, asset, exe), dest, exe);
}

// exeName is the program's file name on this machine.
function exeName(product: string): string {
  return process.platform === 'win32' ? `${product}.exe` : product;
}

// binaryIn is the unpacked binary: cargo-dist puts it at the top of a .zip and in one
// directory named for the archive in a .tar.gz (both looked for). It must be a plain file.
async function binaryIn(into: string, asset: string, exe: string): Promise<string> {
  const stem = asset.replace(/\.(zip|tar\.gz)$/, '');
  for (const path of [join(into, exe), join(into, stem, exe)]) {
    if ((await lstat(path).catch(() => undefined))?.isFile()) return path;
  }
  throw new Error(`${asset} holds no ${exe}; nothing was installed`);
}

// refuseLinks fails when the unpacked tree holds a symbolic link anywhere: a link could make
// chmod or the move reach a file outside it. The release never has one.
async function refuseLinks(dir: string, asset: string): Promise<void> {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) throw new Error(`${asset} holds a link; nothing was installed`);
    if (entry.isDirectory()) await refuseLinks(join(dir, entry.name), asset);
  }
}

// movedInto makes the binary executable and moves it into dest. When the move fails because
// another window has just installed the same version and runs it (Windows will not replace a
// running file), that binary is used.
async function movedInto(found: string, dest: string, exe: string): Promise<string> {
  const target = join(dest, exe);
  if (process.platform !== 'win32') await chmod(found, 0o755);
  await mkdir(dest, { recursive: true });
  try {
    await rename(found, target);
  } catch (err) {
    if (!(await lstat(target).catch(() => undefined))?.isFile()) throw err;
  }
  return target;
}

// removeLeftovers deletes the binaries other extension versions downloaded and scratch
// directories over an hour old. A binary in use stays until next time on Windows; on Unix an
// older version's window keeps running its server, and only a folder it adds later fails to
// start until that window is reloaded.
async function removeLeftovers(storage: string, version: string): Promise<void> {
  const bin = join(storage, 'bin');
  for (const entry of await readdir(bin)) {
    const path = join(bin, entry);
    const stale = entry.startsWith('.download-') && Date.now() - ((await stat(path).catch(() => undefined))?.mtimeMs ?? Date.now()) > STALE_SCRATCH_MS;
    if ((entry !== version && !entry.startsWith('.')) || stale) {
      await rm(path, { recursive: true, force: true }).catch(() => undefined);
    }
  }
}
