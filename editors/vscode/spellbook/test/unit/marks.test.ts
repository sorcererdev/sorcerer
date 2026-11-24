import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { holeRanges } from '../../src/marks';

const range = (line: number, from: number, to: number) => ({ start: { line, character: from }, end: { line, character: to } });

test('only_spellbook_hole_marks_once_each_in_document_order', () => {
  const diagnostics = [
    { source: 'spellbook', code: 'hole', range: range(29, 8, 40) },
    { source: 'spellbook', code: 'commands-use-ports', range: range(4, 0, 14) },
    { source: 'spellbook', code: 'hole', range: range(9, 16, 22) },
    { source: 'scry', code: 'hole', range: range(1, 0, 1) },
    { source: 'spellbook', code: 'hole', range: range(9, 4, 14) },
    { source: 'spellbook', code: 'hole', range: range(9, 4, 14) },
  ];
  assert.deepEqual(holeRanges(diagnostics), [range(9, 4, 14), range(9, 16, 22), range(29, 8, 40)]);
});
