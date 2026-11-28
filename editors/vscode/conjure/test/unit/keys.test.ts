import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { actionTitled, KEYS, UNDO_LAST } from '../../src/keys';

const manifest = JSON.parse(readFileSync(join(__dirname, '../../../package.json'), 'utf8'));
const { commands, keybindings } = manifest.contributes;

test('the_action_with_that_exact_title_is_found', () => {
  const actions = [{ title: 'conjure: Undo' }, { title: 'conjure: Show why' }];
  assert.equal(actionTitled(actions, 'conjure: Show why'), actions[1]);
  assert.equal(actionTitled(actions, 'conjure: Try harder'), undefined);
});

test('package_json_binds_each_chord_to_its_command_in_an_editor', () => {
  for (const { command, keys } of KEYS) {
    assert.ok(commands.some((c: { command: string }) => c.command === command), command);
    const bound = keybindings.filter((b: { command: string }) => b.command === command);
    assert.deepEqual(bound, keys.map(key => ({ command, key, when: 'editorTextFocus' })));
  }
  assert.deepEqual(KEYS.flatMap(k => k.keys), ['ctrl+m n', 'ctrl+m c n', 'ctrl+m c t', 'ctrl+m c u', 'ctrl+m c w']);
});

test('ctrl_m_u_undoes_conjures_change_when_it_is_the_last_or_none_is', () => {
  assert.deepEqual(keybindings.filter((b: { command: string }) => b.command === UNDO_LAST), [
    { command: UNDO_LAST, key: 'ctrl+m u', when: 'editorTextFocus && sorcerer.lastChange == conjure' },
    { command: UNDO_LAST, key: 'ctrl+m u', when: 'editorTextFocus && !sorcerer.lastChange' },
  ]);
});

test('the_commands_are_in_the_conjure_category_with_plain_titles', () => {
  for (const c of commands) {
    assert.equal(c.category, 'conjure');
    assert.doesNotMatch(c.title, /sorcerer|conjure:/i);
  }
  assert.equal(commands.find((c: { command: string }) => c.command === 'conjure.keys.tryHarder').title, 'Try the change again with more effort');
});
