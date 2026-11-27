import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { candidates } from '../../src/binary';

const STORE = '/store/bin/0.1.0';

test('lackey_is_looked_for_in_the_setting_then_path_cargo_local_storage', () => {
  assert.deepEqual(candidates('lackey', '/opt/lackey', { PATH: '/a:/b' }, '/home/me', 'linux', STORE),
    ['/opt/lackey', '/a/lackey', '/b/lackey', '/home/me/.cargo/bin/lackey', '/home/me/.local/bin/lackey',
      '/store/bin/0.1.0/lackey']);
});

test('windows_looks_for_lackey_exe', () => {
  assert.deepEqual(candidates('lackey', '', { Path: 'C:\\tools' }, 'C:\\Users\\me', 'win32', 'C:\\store'),
    ['C:\\tools\\lackey.exe', 'C:\\Users\\me\\.cargo\\bin\\lackey.exe',
      'C:\\Users\\me\\.local\\bin\\lackey.exe', 'C:\\store\\lackey.exe']);
});
