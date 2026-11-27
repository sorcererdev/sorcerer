import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { dirtyText, fixArgument, fixedText, fixRequest, notShown, preferredFix } from '../../src/fix';
import { holesOnEndLine } from '../../src/holes';

const argument = {
  generator: 'use-port', inputs: { app: 'bistro', unit: 'close_table' }, missing: ['port'],
  fix: { file: 'src/a/close_table.py', line: 4, rule: 'commands-use-ports', index: 1, label: 'Use a port the service has', version: null },
};

test('the_code_actions_argument_is_read_and_a_malformed_one_is_refused', () => {
  assert.deepEqual(fixArgument(argument), argument);
  assert.equal(fixArgument(undefined), undefined);
  assert.equal(fixArgument({ generator: 'x' }), undefined);
  assert.equal(fixArgument({ ...argument, fix: { ...argument.fix, line: 'four' } }), undefined);
});

test('the_plan_gets_the_fix_with_the_buffers_version', () => {
  assert.deepEqual(fixRequest(argument.fix, 12),
    { file: 'src/a/close_table.py', line: 4, rule: 'commands-use-ports', index: 1, version: 12 });
});

test('a_dirty_buffer_is_refused_by_name', () => {
  assert.equal(dirtyText('src/a/close_table.py'),
    'spellbook: save close_table.py first; a fix is planned against the file on disk');
});

test('the_preferred_quick_fix_is_the_first_spellbook_fix_action', () => {
  const other = { title: 'Conjure here', command: { command: 'conjure.conjure', arguments: [] } };
  const first = { title: 'spellbook: A', isPreferred: true, command: { command: 'spellbook.fix', arguments: [argument] } };
  const second = { title: 'spellbook: B', command: { command: 'spellbook.fix', arguments: [argument] } };
  assert.equal(preferredFix([other, second, first]), first);
  assert.equal(preferredFix([other, second]), second);
  assert.equal(preferredFix([other]), undefined);
});

test('the_status_line_says_what_the_fix_changed_wrote_and_left_to_fill', () => {
  assert.equal(fixedText('commands-use-ports', 'src/a/close_table.py', 2, 0, 0),
    'spellbook: fixed commands-use-ports in close_table.py · changed 2 files');
  assert.equal(fixedText('commands-use-ports', 'src/a/close_table.py', 2, 4, 2),
    'spellbook: fixed commands-use-ports in close_table.py · changed 2 files · wrote 4 files · 2 holes, marked');
  assert.equal(fixedText('r', 'a.py', 1, 0, 1), 'spellbook: fixed r in a.py · changed 1 file · 1 hole, marked');
});

test('the_status_line_names_a_changed_file_no_tab_shows_since_it_is_left_unsaved', () => {
  assert.equal(fixedText('r', 'src/a/close_table.py', 2, 0, 0, ['src/a/close_table__test.py']),
    'spellbook: fixed r in close_table.py · changed 2 files · close_table__test.py not shown, unsaved');
  assert.equal(fixedText('r', 'a.py', 6, 0, 0, ['p/b.py', 'c.py', 'd.py', 'e.py', 'f.py']),
    'spellbook: fixed r in a.py · changed 6 files · b.py, c.py, d.py and 2 more not shown, unsaved');
});

test('the_changed_files_not_shown_are_those_no_tab_holds', () => {
  assert.deepEqual(notShown(['src/a.py', 'src/b.py'], path => path === 'src/a.py'), ['src/b.py']);
  assert.deepEqual(notShown([], () => false), []);
});

test('a_started_hole_session_says_the_holes_itself_so_the_end_line_counts_none', () => {
  assert.equal(holesOnEndLine(true, 3), 0);
  assert.equal(holesOnEndLine(false, 3), 3);
});
