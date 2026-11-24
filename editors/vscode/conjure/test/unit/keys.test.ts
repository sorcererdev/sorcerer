import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { actionTitled, KEYS } from '../../src/keys';

test('the_action_with_that_exact_title_is_found', () => {
  const actions = [{ title: 'conjure: Undo' }, { title: 'conjure: Show why' }];
  assert.equal(actionTitled(actions, 'conjure: Show why'), actions[1]);
  assert.equal(actionTitled(actions, 'conjure: Try harder'), undefined);
});

test('package_json_binds_the_four_chords_to_the_four_commands', () => {
  const manifest = JSON.parse(readFileSync(join(__dirname, '../../../package.json'), 'utf8'));
  const { commands, keybindings } = manifest.contributes;
  for (const { command, key } of KEYS) {
    assert.ok(commands.some((c: { command: string }) => c.command === command), command);
    const binding = keybindings.find((b: { command: string }) => b.command === command);
    assert.equal(binding?.key, key);
    assert.equal(binding?.when, 'editorTextFocus');
  }
  assert.deepEqual(KEYS.map(k => k.key), ['ctrl+m c', 'ctrl+m u', 'ctrl+m t', 'ctrl+m w']);
});
