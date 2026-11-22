import { join } from 'node:path';
import Mocha from 'mocha';

// run loads conjure.test.js beside this file into mocha inside VS Code.
export function run(): Promise<void> {
  const mocha = new Mocha({ ui: 'bdd', timeout: 300_000, color: true });
  mocha.addFile(join(__dirname, 'conjure.test.js'));
  return new Promise((res, rej) => mocha.run(failures => (failures ? rej(new Error(`${failures} failed`)) : res())));
}
