import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { afterChanges, type Ghost, ghostText, parseGhost, withGhost } from '../../src/ghost';

const URI = 'file:///p/a.py';
const UNDER_4: Ghost = { uri: URI, line: 4, lines: ['    return 0'] };

test('a_ghost_is_read_from_its_params_and_malformed_ones_are_nothing', () => {
  assert.deepEqual(parseGhost({ uri: URI, line: 4, lines: ['    return 0', 7] }), UNDER_4);
  assert.deepEqual(parseGhost({ uri: URI, line: 4 }), { uri: URI, line: 4, lines: [] });
  assert.equal(parseGhost({ uri: URI, line: -1, lines: [] }), undefined);
  assert.equal(parseGhost({ line: 4, lines: [] }), undefined);
  assert.equal(parseGhost(null), undefined);
});

test('a_ghost_replaces_the_one_on_its_line_and_no_lines_takes_it_away', () => {
  const other = { uri: URI, line: 9, lines: ['x'] };
  const replaced = withGhost([UNDER_4, other], { uri: URI, line: 4, lines: ['    return 1'] });
  assert.deepEqual(replaced, [other, { uri: URI, line: 4, lines: ['    return 1'] }]);
  assert.deepEqual(withGhost(replaced, { uri: URI, line: 4, lines: [] }), [other]);
});

test('typing_on_the_ghosts_line_takes_it_away', () => {
  assert.deepEqual(afterChanges([UNDER_4], [{ startLine: 4, endLine: 4, endCharacter: 3, text: 'x' }]), []);
});

test('an_edit_above_moves_the_ghost_by_the_lines_it_added_or_took_out', () => {
  const enter = { startLine: 1, endLine: 1, endCharacter: 2, text: '\n' };
  assert.equal(afterChanges([UNDER_4], [enter])[0].line, 5);
  const twoLinesGone = { startLine: 1, endLine: 3, endCharacter: 0, text: '' };
  assert.equal(afterChanges([UNDER_4], [twoLinesGone])[0].line, 2);
});

test('an_edit_below_leaves_the_ghost_where_it_is', () => {
  assert.deepEqual(afterChanges([UNDER_4], [{ startLine: 6, endLine: 8, endCharacter: 0, text: '' }]), [UNDER_4]);
});

test('the_ghost_is_written_after_its_line_without_indentation_and_cut_at_120', () => {
  assert.equal(ghostText(['    if late:', '        raise TooLate()']), '⤷ if late: ⏎ raise TooLate()');
  const long = ghostText(['x'.repeat(200)]);
  assert.equal(long.length, 120);
  assert.ok(long.endsWith('…'));
});
