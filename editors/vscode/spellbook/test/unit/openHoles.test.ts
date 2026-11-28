import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { holeLabel, openHoles } from '../../src/openHoles';

test('the_open_holes_are_read_and_listed_with_what_they_want', () => {
  const holes = openHoles('[{"name":"field","file":"src/a.py","line":3,"kind":"expr","instruction":"its first input"}]');
  assert.deepEqual(holes, [{ name: 'field', file: 'src/a.py', line: 3, instruction: 'its first input' }]);
  assert.equal(holeLabel((holes as never)[0]), 'field · src/a.py:3 · its first input');
});

test('something_unreadable_is_said', () => {
  assert.equal(openHoles('nope'), 'spellbook: holes printed something unreadable');
  assert.equal(openHoles('{}'), 'spellbook: holes printed something unreadable');
});
