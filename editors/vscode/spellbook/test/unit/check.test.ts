import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { checkFailedText, findingEntries, parseReport } from '../../src/check';

const finding = (file: string, severity: string, line = 5) =>
  ({ rule: 'commands-use-ports', file, line, severity, message: 'Commands talk to ports' });

test('the_report_is_read_and_anything_else_is_said', () => {
  assert.deepEqual(parseReport('{"checked":"x","findings":[]}'), []);
  assert.deepEqual(parseReport(JSON.stringify({ findings: [finding('a.py', 'error')] })), [finding('a.py', 'error')]);
  assert.equal(parseReport('oops'), 'spellbook: check printed something unreadable');
  assert.equal(parseReport('[1]'), 'spellbook: check printed something unreadable');
});

test('severities_map_to_vscodes_and_lines_count_from_zero', () => {
  assert.deepEqual(findingEntries([finding('a.py', 'error'), finding('b.py', 'warn', 1), finding('c.py', 'hint')], new Set()), [
    { file: 'a.py', line: 4, severity: 'error', code: 'commands-use-ports', message: 'Commands talk to ports' },
    { file: 'b.py', line: 0, severity: 'warning', code: 'commands-use-ports', message: 'Commands talk to ports' },
    { file: 'c.py', line: 4, severity: 'hint', code: 'commands-use-ports', message: 'Commands talk to ports' },
  ]);
});

test('an_open_file_is_left_to_the_server', () => {
  assert.deepEqual(findingEntries([finding('src/a.py', 'error'), finding('src/b.py', 'error')], new Set(['src/a.py'])).map(e => e.file), ['src/b.py']);
});

test('a_check_that_could_not_run_says_its_error_output', () => {
  assert.equal(checkFailedText('spellbook is off\n', 'x'), 'spellbook: check could not run: spellbook is off');
  assert.equal(checkFailedText('  ', 'no project here\n'), 'spellbook: check could not run: no project here');
});
