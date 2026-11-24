import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { candidates } from '../../src/binary';

const STORE = '/store/bin/0.1.0';

test('spellbook_is_looked_for_in_the_setting_then_path_cargo_local_storage', () => {
  assert.deepEqual(candidates('spellbook', '/opt/spellbook', { PATH: '/a:/b' }, '/home/me', 'linux', STORE),
    ['/opt/spellbook', '/a/spellbook', '/b/spellbook', '/home/me/.cargo/bin/spellbook', '/home/me/.local/bin/spellbook',
      '/store/bin/0.1.0/spellbook']);
});

test('windows_looks_for_spellbook_exe', () => {
  assert.deepEqual(candidates('spellbook', '', { Path: 'C:\\tools' }, 'C:\\Users\\me', 'win32', 'C:\\store'),
    ['C:\\tools\\spellbook.exe', 'C:\\Users\\me\\.cargo\\bin\\spellbook.exe',
      'C:\\Users\\me\\.local\\bin\\spellbook.exe', 'C:\\store\\spellbook.exe']);
});
