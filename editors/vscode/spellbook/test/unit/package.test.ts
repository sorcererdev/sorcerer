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
