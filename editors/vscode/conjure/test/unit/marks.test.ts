import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { conjuredLines } from '../../src/marks';

test('only_a_conjure_change_mark_is_a_conjured_line', () => {
  const lines = conjuredLines([
    { source: 'conjure', code: 'change', line: 7 },
    { source: 'conjure', code: 'no-change', line: 2 },
    { source: 'scry', code: 'change', line: 3 },
    { source: 'conjure', code: 'change', line: 7 },
    { source: 'conjure', code: 'change', line: 1 },
  ]);
  assert.deepEqual(lines, [1, 7]);
});
