import { join } from 'node:path';
import Mocha from 'mocha';

// run loads the suite run.ts launched this VS Code for (SCRY_E2E_SUITE: `scry` loads
// scry.test.js beside this file) into mocha inside VS Code.
export function run(): Promise<void> {
  const suite = process.env.SCRY_E2E_SUITE ?? 'scry';
  const mocha = new Mocha({ ui: 'bdd', timeout: 300_000, color: true });
  mocha.addFile(join(__dirname, `${suite}.test.js`));
  return new Promise((res, rej) => mocha.run(failures => (failures ? rej(new Error(`${failures} failed`)) : res())));
}
