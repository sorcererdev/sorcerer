import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

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

test('the_extension_is_sorcererdev_spellbook_of_the_workspace_version', () => {
  assert.equal(manifest.name, 'spellbook');
  assert.equal(manifest.publisher, 'sorcererdev');
  assert.equal(manifest.version, workspaceVersion());
});

test('the_binary_setting_is_read_from_user_settings_only_and_not_in_restricted_mode', () => {
  assert.equal(settings['spellbook.path'].scope, 'machine');
  assert.deepEqual(manifest.capabilities.untrustedWorkspaces.restrictedConfigurations, ['spellbook.path']);
});

// the keybindings of a command, as the manifest has them
function bindings(command: string): { command: string; key: string; when: string }[] {
  return manifest.contributes.keybindings.filter((k: { command: string }) => k.command === command);
}

test('each_command_has_its_chords_under_ctrl_m_in_an_editor_only', () => {
  const expected: Record<string, string[]> = {
    'spellbook.new': ['ctrl+m g', 'ctrl+m b g'],
    'spellbook.fixThis': ['ctrl+m f'],
    'spellbook.fixHere': ['ctrl+m b f'],
    'spellbook.check': ['ctrl+m b k'],
    'spellbook.sync': ['ctrl+m b s'],
    'spellbook.holes': ['ctrl+m b h'],
  };
  for (const [command, keys] of Object.entries(expected)) {
    assert.deepEqual(bindings(command), keys.map(key => ({ command, key, when: 'editorTextFocus' })), command);
  }
  assert.deepEqual(manifest.contributes.commands.map((c: { command: string }) => c.command), [...Object.keys(expected), 'spellbook.undoLast']);
});

test('ctrl_m_u_undoes_spellbooks_change_when_it_is_the_last_or_none_is', () => {
  assert.deepEqual(bindings('spellbook.undoLast'), [
    { command: 'spellbook.undoLast', key: 'ctrl+m u', when: 'editorTextFocus && sorcerer.lastChange == spellbook' },
    { command: 'spellbook.undoLast', key: 'ctrl+m u', when: 'editorTextFocus && !sorcerer.lastChange' },
  ]);
});

test('the_commands_are_in_the_spellbook_category_with_plain_titles', () => {
  for (const c of manifest.contributes.commands) {
    assert.equal(c.category, 'spellbook');
    assert.doesNotMatch(c.title, /sorcerer|spellbook:/i);
  }
  assert.equal(manifest.contributes.commands[0].title, 'Generate…');
});

test('conjure_on_the_first_hole_is_off_unless_asked', () => {
  assert.equal(settings['spellbook.conjureHoles'].type, 'boolean');
  assert.equal(settings['spellbook.conjureHoles'].default, false);
});

test('the_hole_session_is_on_unless_turned_off', () => {
  assert.equal(settings['spellbook.holeSession'].type, 'boolean');
  assert.equal(settings['spellbook.holeSession'].default, true);
});
