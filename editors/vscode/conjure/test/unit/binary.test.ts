import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { candidates } from '../../src/binary';

const STORE = '/store/bin/0.1.0';

test('conjure_is_looked_for_in_the_setting_then_path_cargo_local_storage', () => {
  assert.deepEqual(candidates('conjure', '/opt/conjure', { PATH: '/a:/b' }, '/home/me', 'linux', STORE),
    ['/opt/conjure', '/a/conjure', '/b/conjure', '/home/me/.cargo/bin/conjure', '/home/me/.local/bin/conjure',
      '/store/bin/0.1.0/conjure']);
});

test('windows_looks_for_conjure_exe', () => {
  assert.deepEqual(candidates('conjure', '', { Path: 'C:\\tools' }, 'C:\\Users\\me', 'win32', 'C:\\store'),
    ['C:\\tools\\conjure.exe', 'C:\\Users\\me\\.cargo\\bin\\conjure.exe',
      'C:\\Users\\me\\.local\\bin\\conjure.exe', 'C:\\store\\conjure.exe']);
});
