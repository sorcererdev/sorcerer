import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { candidates, find } from '../../src/binary';

const STORE = '/store/bin/0.1.0';

test('the setting comes first', () => {
  assert.deepEqual(candidates('/opt/scry', { PATH: '/usr/bin' }, '/home/me', 'linux', STORE),
    ['/opt/scry', '/usr/bin/scry', '/home/me/.cargo/bin/scry', '/home/me/.local/bin/scry', '/store/bin/0.1.0/scry']);
});

test('the_order_is_setting_path_cargo_local_storage', () => {
  assert.deepEqual(candidates('', { PATH: '/a:/b' }, '/home/me', 'darwin', STORE),
    ['/a/scry', '/b/scry', '/home/me/.cargo/bin/scry', '/home/me/.local/bin/scry', '/store/bin/0.1.0/scry']);
});

test('windows_looks_for_scry_exe', () => {
  assert.deepEqual(
    candidates('', { Path: 'C:\\tools;C:\\bin' }, 'C:\\Users\\José Ñ', 'win32', 'C:\\store\\bin\\0.1.0'),
    ['C:\\tools\\scry.exe', 'C:\\bin\\scry.exe', 'C:\\Users\\José Ñ\\.cargo\\bin\\scry.exe',
      'C:\\Users\\José Ñ\\.local\\bin\\scry.exe', 'C:\\store\\bin\\0.1.0\\scry.exe']);
});

test('an empty setting is skipped', () => {
  assert.ok(!candidates('', { PATH: '/a' }, '/h', 'linux', STORE).includes(''));
  assert.ok(!candidates('   ', { PATH: '/a' }, '/h', 'linux', STORE).includes('   '));
  assert.equal(candidates('', {}, '/h', 'linux', STORE)[0], '/h/.cargo/bin/scry');
});

test('find returns the first that exists, or undefined', () => {
  const c = ['/a/scry', '/b/scry', '/c/scry'];
  assert.equal(find(c, p => p !== '/a/scry'), '/b/scry');
  assert.equal(find(c, () => false), undefined);
  assert.equal(find([], () => true), undefined);
});

// A workspace's .vscode/settings.json must not choose the program the extension runs: VS Code
// reads a machine-scoped setting from user settings only, and Restricted Mode withholds it.
test('scry_path_is_never_read_from_a_workspace', () => {
  const pkg = JSON.parse(readFileSync(join(__dirname, '../../../package.json'), 'utf8'));
  assert.equal(pkg.contributes.configuration.properties['scry.path'].scope, 'machine');
  assert.deepEqual(pkg.capabilities.untrustedWorkspaces,
    { supported: 'limited', restrictedConfigurations: ['scry.path'], description: pkg.capabilities.untrustedWorkspaces.description });
});
