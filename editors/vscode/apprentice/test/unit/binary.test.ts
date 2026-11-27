import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { candidates } from '../../src/binary';

const STORE = '/store/bin/0.1.0';

test('apprentice_is_looked_for_in_the_setting_then_path_cargo_local_storage', () => {
  assert.deepEqual(candidates('apprentice', '/opt/apprentice', { PATH: '/a:/b' }, '/home/me', 'linux', STORE),
    ['/opt/apprentice', '/a/apprentice', '/b/apprentice', '/home/me/.cargo/bin/apprentice', '/home/me/.local/bin/apprentice',
      '/store/bin/0.1.0/apprentice']);
});

test('windows_looks_for_apprentice_exe', () => {
  assert.deepEqual(candidates('apprentice', '', { Path: 'C:\\tools' }, 'C:\\Users\\me', 'win32', 'C:\\store'),
    ['C:\\tools\\apprentice.exe', 'C:\\Users\\me\\.cargo\\bin\\apprentice.exe',
      'C:\\Users\\me\\.local\\bin\\apprentice.exe', 'C:\\store\\apprentice.exe']);
});
