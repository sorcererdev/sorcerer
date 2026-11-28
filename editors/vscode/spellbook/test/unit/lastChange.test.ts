import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { nearestLine, placesOf, undoneText } from '../../src/lastChange';

const at = (line: number) => ({ range: { start: { line } } });

test('an_edit_starts_at_the_first_changed_line_of_each_file_once', () => {
  const places = placesOf({
    changes: { 'file:///a.py': [at(4), at(2)] },
    documentChanges: [{ textDocument: { uri: 'file:///c.py' }, edits: [at(7)] }, { kind: 'create', uri: 'file:///b.py' }],
  });
  assert.deepEqual(places, [{ uri: 'file:///a.py', line: 2 }, { uri: 'file:///c.py', line: 7 }, { uri: 'file:///b.py', line: 0 }]);
  assert.deepEqual(placesOf(undefined), []);
});

test('the_nearest_mark_is_found', () => {
  assert.equal(nearestLine([3, 40], 10), 3);
  assert.equal(nearestLine([], 10), undefined);
});

test('the_undo_says_whose_change_and_where', () => {
  assert.equal(undoneText('conjure', 'a.py', 11), "undo: conjure's last change (a.py:12)");
});
