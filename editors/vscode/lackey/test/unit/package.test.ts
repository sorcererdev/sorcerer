import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { KEYS, UNDO_LAST } from '../../src/keys';

// the tests run from out/test/unit: the extension's folder is three up, the workspace five up
const EXTENSION = join(__dirname, '..', '..', '..');
const manifest = JSON.parse(readFileSync(join(EXTENSION, 'package.json'), 'utf8'));
const settings = manifest.contributes.configuration.properties;

// workspaceVersion is the `version = "…"` line of the workspace's Cargo.toml: the extension
// ships with the release of the same version.
function workspaceVersion(): string {
  const cargo = readFileSync(join(EXTENSION, '..', '..', '..', 'Cargo.toml'), 'utf8');
  const found = /^version\s*=\s*"([^"]+)"/m.exec(cargo);
  assert.ok(found, 'the workspace Cargo.toml names a version');
  return found[1];
}

test('the_extension_is_sorcererdev_lackey_of_the_workspace_version', () => {
  assert.equal(manifest.name, 'lackey');
  assert.equal(manifest.publisher, 'sorcererdev');
  assert.equal(manifest.version, workspaceVersion());
});

test('the_binary_setting_is_read_from_user_settings_only_and_not_in_restricted_mode', () => {
  assert.equal(settings['lackey.path'].scope, 'machine');
  assert.deepEqual(manifest.capabilities.untrustedWorkspaces.restrictedConfigurations, ['lackey.path']);
});

test('each_chord_calls_its_command_in_an_editor_only', () => {
  const { commands, keybindings } = manifest.contributes;
  for (const { command, keys } of KEYS) {
    assert.ok(commands.some((c: { command: string }) => c.command === command), command);
    assert.deepEqual(keybindings.filter((b: { command: string }) => b.command === command), keys.map(key => ({ command, key, when: 'editorTextFocus' })));
  }
  assert.equal(keybindings.length, KEYS.flatMap(k => k.keys).length + 2);
});

test('ctrl_m_u_undoes_lackeys_task_when_it_is_the_last_change_or_none_is', () => {
  assert.deepEqual(manifest.contributes.keybindings.filter((b: { command: string }) => b.command === UNDO_LAST), [
    { command: UNDO_LAST, key: 'ctrl+m u', when: 'editorTextFocus && sorcerer.lastChange == lackey' },
    { command: UNDO_LAST, key: 'ctrl+m u', when: 'editorTextFocus && !sorcerer.lastChange' },
  ]);
});

test('the_commands_are_in_the_lackey_category_with_plain_titles', () => {
  for (const c of manifest.contributes.commands) {
    assert.equal(c.category, 'lackey');
    assert.doesNotMatch(c.title, /sorcerer|lackey:/i);
  }
});
