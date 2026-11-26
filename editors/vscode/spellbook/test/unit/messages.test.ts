import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { messagePlace } from '../../src/messages';

test('a_saves_sync_line_goes_to_the_status_bar', () => {
  assert.equal(messagePlace(3, 'spellbook: synced region modules in src/booking/commands/mod.rs (+1 −0)'), 'status');
});

test('every_other_message_keeps_its_level', () => {
  assert.equal(messagePlace(3, 'spellbook: 1 region out of date; run spellbook sync'), 'info');
  assert.equal(messagePlace(2, 'spellbook: synced region modules in a (+1 −0)'), 'warning');
  assert.equal(messagePlace(1, 'spellbook: broken'), 'error');
});
