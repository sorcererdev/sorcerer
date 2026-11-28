import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { actionsFor, KEYS, keysByName } from '../../src/keys';

const LINE = [
  { title: 'lackey: do this · ~$0.50 · sonnet' },
  { title: 'lackey: Dismiss' },
  { title: 'lackey: this was a question' },
  { title: 'lackey: this was a claim' },
  { title: 'lackey: this was an instruction' },
  { title: 'lackey: this was nothing' },
];

function key(command: string) {
  const found = KEYS.find(k => k.command === command);
  assert.ok(found, command);
  return found;
}

test('do_this_runs_the_offer_whatever_its_price', () => {
  assert.deepEqual(actionsFor(LINE, key('lackey.keys.do')), [LINE[0]]);
});

test('an_exact_key_runs_only_the_action_of_that_title', () => {
  assert.deepEqual(actionsFor(LINE, key('lackey.keys.dismiss')), [LINE[1]]);
  assert.deepEqual(actionsFor(LINE, key('lackey.keys.undoTask')), []);
});

test('the_label_key_offers_the_four_labels_to_choose_from', () => {
  const label = key('lackey.keys.label');
  assert.equal(label.choose, true);
  assert.deepEqual(actionsFor(LINE, label).map(a => a.title), LINE.slice(2).map(a => a.title));
});

test('the_keys_are_neovims_letters_under_ctrl_m', () => {
  assert.deepEqual(KEYS.flatMap(k => k.keys), ['ctrl+m a', 'ctrl+m l a', 'ctrl+m q', 'ctrl+m l q', 'ctrl+m l d', 'ctrl+m l z', 'ctrl+m l j', 'ctrl+m l o', 'ctrl+m l x', 'ctrl+m l l']);
});

test('the_server_is_told_the_open_answer_chord_by_name', () => {
  assert.deepEqual(keysByName(KEYS), { openAnswer: 'ctrl+m l o' });
});
