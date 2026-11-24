import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { endText } from '../../src/holes';

test('the_end_line_names_conjure_when_it_is_here_else_the_marks', () => {
  assert.equal(endText(2, 3, true), 'spellbook: wrote 2 files · 3 holes · ctrl+m c to conjure the first');
  assert.equal(endText(2, 3, false), 'spellbook: wrote 2 files · 3 holes, marked (Problems)');
  assert.equal(endText(1, 1, false), 'spellbook: wrote 1 file · 1 hole, marked (Problems)');
  assert.equal(endText(2, 0, true), 'spellbook: wrote 2 files');
});
