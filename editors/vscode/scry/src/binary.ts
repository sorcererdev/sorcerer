import { constants, accessSync, statSync } from 'node:fs';
import { delimiter, join } from 'node:path';

// candidates lists where the scry binary may be, best first: the `scry.path` setting when it
// is not empty, then `scry` in each PATH directory, then ~/.cargo/bin (where `cargo install`
// puts it, and which a VS Code opened from the Dock may not have on its PATH).
export function candidates(setting: string, env: NodeJS.ProcessEnv, home: string): string[] {
  const out: string[] = [];
  if (setting.trim() !== '') out.push(setting.trim());
  for (const dir of (env.PATH ?? '').split(delimiter)) if (dir !== '') out.push(join(dir, 'scry'));
  out.push(join(home, '.cargo', 'bin', 'scry'));
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
