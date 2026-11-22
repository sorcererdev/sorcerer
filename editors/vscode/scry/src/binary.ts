import { constants, accessSync, statSync } from 'node:fs';
import { posix, win32 } from 'node:path';

// candidates lists where a product's binary (`scry`, `conjure`) may be, best first: the
// product's path setting when it is not empty, then the product (`scry.exe` on Windows) in each
// PATH directory, then ~/.cargo/bin
// (where `cargo install` puts it), ~/.local/bin (where the installers put it; a VS Code
// opened from the Dock may have neither on its PATH), and last `storage`, the directory where
// this extension downloads the binary of its own version.
//
// Example: candidates('scry', '', { PATH: '/a:/b' }, '/home/me', 'linux', '/store/bin/0.1.0') is
// ['/a/scry', '/b/scry', '/home/me/.cargo/bin/scry', '/home/me/.local/bin/scry',
// '/store/bin/0.1.0/scry'].
export function candidates(
  product: string,
  setting: string,
  env: NodeJS.ProcessEnv,
  home: string,
  platform: NodeJS.Platform,
  storage: string,
): string[] {
  const path = platform === 'win32' ? win32 : posix;
  const name = platform === 'win32' ? `${product}.exe` : product;
  // Windows spells the variable Path; a copy of process.env no longer ignores the case
  const dirs = (env.PATH ?? env.Path ?? '').split(path.delimiter).filter(dir => dir !== '');
  const out = [
    ...(setting.trim() !== '' ? [setting.trim()] : []),
    ...dirs.map(dir => path.join(dir, name)),
    path.join(home, '.cargo', 'bin', name),
    path.join(home, '.local', 'bin', name),
    path.join(storage, name),
  ];
  return [...new Set(out)];
}

// find is the first candidate that exists, or undefined.
export function find(c: string[], exists: (p: string) => boolean): string | undefined {
  return c.find(exists);
}

// isExecutable: a file this user may run.
export function isExecutable(p: string): boolean {
  try {
    accessSync(p, constants.X_OK);
    return statSync(p).isFile();
  } catch {
    return false;
  }
}
