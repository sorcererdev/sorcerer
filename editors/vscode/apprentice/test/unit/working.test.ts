import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { forDocument, parseWorking } from '../../src/working';

test('the_units_of_a_notification_are_read_in_order', () => {
  const params = { units: [{ uri: 'file:///a.py', line: 3, text: '⟳ scry: reviewing…' }, { uri: 'file:///b.py', line: 0, text: 'x' }] };
  assert.deepEqual(parseWorking(params), params.units);
});

test('an_empty_list_is_nothing_drawn', () => {
  assert.deepEqual(parseWorking({ units: [] }), []);
});

test('anything_that_is_not_a_unit_is_left_out', () => {
  const params = { units: [{ uri: 'file:///a.py', line: -1, text: 'x' }, { uri: 'file:///a.py', line: 1.5, text: 'x' },
    { uri: 5, line: 1, text: 'x' }, null, { uri: 'file:///a.py', line: 2, text: 'ok' }] };
  assert.deepEqual(parseWorking(params), [{ uri: 'file:///a.py', line: 2, text: 'ok' }]);
});

test('params_that_are_not_a_list_of_units_are_nothing', () => {
  for (const params of [undefined, null, 'x', {}, { units: 'x' }]) assert.deepEqual(parseWorking(params), []);
});

test('the_units_of_one_document_are_picked_by_its_uri', () => {
  const units = [{ uri: 'file:///a.py', line: 1, text: 't' }, { uri: 'file:///b.py', line: 2, text: 't' }];
  assert.deepEqual(forDocument(units, 'file:///b.py'), [units[1]]);
  assert.deepEqual(forDocument(units, 'file:///c.py'), []);
});
