import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { taskLines } from '../../src/marks';

test('only_a_finished_tasks_mark_is_a_task_line', () => {
  const lines = taskLines([
    { source: 'apprentice', code: 'task', line: 7 },
    { source: 'apprentice', code: 'region', line: 2 },
    { source: 'apprentice', code: 'said', line: 3 },
    { source: 'apprentice', code: 'ask', line: 4 },
    { source: 'conjure', code: 'task', line: 5 },
    { source: 'apprentice', code: 'task', line: 7 },
    { source: 'apprentice', code: 'task', line: 1 },
  ]);
  assert.deepEqual(lines, [1, 7]);
});
