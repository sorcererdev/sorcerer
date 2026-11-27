import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { forDocument, parseWorking, spelledAs } from '../../src/working';

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

test('a_windows_server_uri_is_spelled_as_vs_code_names_the_document', () => {
  const vsCode = (uri: string) => uri.replace('file:///C:/', 'file:///c%3A/');
  const units = [{ uri: 'file:///C:/a.py', line: 1, text: 'x' }];
  const spelled = spelledAs(units, vsCode);
  assert.deepEqual(spelled, [{ uri: 'file:///c%3A/a.py', line: 1, text: 'x' }]);
  assert.equal(forDocument(spelled, 'file:///c%3A/a.py').length, 1);
  assert.equal(forDocument(units, 'file:///c%3A/a.py').length, 0);
});
