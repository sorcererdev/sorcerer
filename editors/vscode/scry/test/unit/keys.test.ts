import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { actionsFor, KEYS, LIST, REVIEW } from '../../src/keys';

const manifest = JSON.parse(readFileSync(join(__dirname, '../../../package.json'), 'utf8'));
const { commands, keybindings } = manifest.contributes;

const LINE = [
  { title: 'scry: Show why' },
  { title: 'scry: Show code' },
  { title: 'scry: Dismiss here' },
  { title: 'scry: Dismiss everywhere' },
];

function key(command: string) {
  const found = KEYS.find(k => k.command === command);
  assert.ok(found, command);
  return found;
}

test('why_runs_show_why_and_dismiss_offers_here_or_everywhere', () => {
  assert.deepEqual(actionsFor(LINE, key('scry.keys.why')), [LINE[0]]);
  const dismiss = key('scry.keys.dismiss');
  assert.equal(dismiss.choose, true);
  assert.deepEqual(actionsFor(LINE, dismiss), LINE.slice(2));
});

test('each_chord_calls_its_command_in_an_editor_only', () => {
  for (const { command, keys } of [...KEYS, REVIEW, LIST]) {
    assert.ok(commands.some((c: { command: string }) => c.command === command), command);
    assert.deepEqual(keybindings.filter((b: { command: string }) => b.command === command), keys.map(k => ({ command, key: k, when: 'editorTextFocus' })));
  }
  assert.deepEqual(keybindings.map((b: { key: string }) => b.key), ['ctrl+m w', 'ctrl+m s w', 'ctrl+m s r', 'ctrl+m s x', 'ctrl+m s l', 'ctrl+m s c']);
});

test('the_commands_are_in_the_scry_category_with_plain_titles', () => {
  for (const c of commands) {
    assert.equal(c.category, 'scry');
    assert.doesNotMatch(c.title, /sorcerer|scry:/i);
  }
  assert.equal(commands.find((c: { command: string }) => c.command === 'scry.keys.why').title, 'Why is this flagged?');
});
