import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseNote, parseShowDiff } from '../../src/diff';

test('show_diff_is_read_with_a_new_file_as_an_empty_before', () => {
  assert.deepEqual(parseShowDiff({ uri: 'file:///p/a.py', before: 'x\n', title: 'lackey: fill tax · before' }),
    { uri: 'file:///p/a.py', before: 'x\n', title: 'lackey: fill tax · before' });
  assert.deepEqual(parseShowDiff({ uri: 'file:///p/new.py' }), { uri: 'file:///p/new.py', before: '', title: 'lackey: before' });
  assert.equal(parseShowDiff({ before: 'x' }), undefined);
});

test('a_note_is_its_uri', () => {
  assert.equal(parseNote({ uri: 'file:///p/.sorcerer/state/lackey/notes/t1-why.md' }), 'file:///p/.sorcerer/state/lackey/notes/t1-why.md');
  assert.equal(parseNote({}), undefined);
});
