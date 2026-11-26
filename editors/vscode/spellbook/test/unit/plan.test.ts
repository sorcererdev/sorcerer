import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { changedFiles, fileItems, newFiles, type Plan, placeholderRanges, planTitle, planUriPath } from '../../src/plan';

const plan: Plan = {
  id: '1a2b3c4d',
  generator: 'command',
  files: [
    { path: 'src/b/commands/__init__.py', status: 'skipped', text: '', lines: 0 },
    { path: 'src/b/commands/cancel_table.py', status: 'new', text: 'x', lines: 31 },
    { path: 'src/b/commands/cancel_table__test.py', status: 'new', text: 'y', lines: 1 },
  ],
  holes: [
    { name: 'field', kind: 'ident', instruction: 'its first input', source: 'placeholder',
      places: [{ file: 'src/b/commands/cancel_table.py', line: 2, text: 'field' },
        { file: 'src/b/commands/cancel_table__test.py', line: 1, text: 'field' }] },
    { name: 'handle', kind: 'block', instruction: 'the body', source: 'placeholder',
      places: [{ file: 'src/b/commands/cancel_table.py', line: 3, text: 'not here' }] },
    { name: 'purpose', kind: 'text', instruction: 'docstring', source: 'derived',
      places: [{ file: 'src/b/commands/cancel_table.py', line: 1, text: 'Cancel table.' }] },
  ],
};

test('the_title_counts_new_files_and_holes_to_fill', () => {
  assert.equal(planTitle(plan), 'spellbook: command · 2 files · 2 holes to fill');
  const one: Plan = { ...plan, files: [plan.files[1]], holes: [plan.holes[0]] };
  assert.equal(planTitle(one), 'spellbook: command · 1 file · 1 hole to fill');
});

test('the_title_names_the_inputs_taken_without_asking', () => {
  assert.equal(planTitle(plan, { app: 'bistro' }), 'spellbook: command · 2 files · 2 holes to fill · app bistro (the only choice)');
  assert.equal(planTitle(plan, { app: 'bistro', service: 'booking' }),
    'spellbook: command · 2 files · 2 holes to fill · app bistro, service booking (the only choices)');
});

test('new_files_say_their_lines_and_skipped_ones_say_so', () => {
  assert.deepEqual(fileItems(plan), [
    { label: '= src/b/commands/__init__.py', description: 'exists, skipped', path: 'src/b/commands/__init__.py' },
    { label: '+ src/b/commands/cancel_table.py', description: '31 lines', path: 'src/b/commands/cancel_table.py' },
    { label: '+ src/b/commands/cancel_table__test.py', description: '1 line', path: 'src/b/commands/cancel_table__test.py' },
  ]);
});

test('a_preview_path_is_the_plan_then_the_file', () => {
  assert.equal(planUriPath('1a2b3c4d', 'src/a.py'), '/1a2b3c4d/src/a.py');
});

test('a_placeholder_is_found_on_its_row_else_the_row_after_its_indent_and_derived_holes_are_not_lit', () => {
  const rows = ['"""Cancel table."""', '    field: type', '    raise NotImplementedError'];
  assert.deepEqual(placeholderRanges(plan, 'src/b/commands/cancel_table.py', row => rows[row]), [
    { line: 1, start: 4, end: 9 },
    { line: 2, start: 4, end: 29 },
  ]);
});

test('an_empty_placeholder_text_lights_the_row', () => {
  const empty: Plan = { ...plan, holes: [{ ...plan.holes[0], places: [{ file: 'a.py', line: 1, text: '' }] }] };
  assert.deepEqual(placeholderRanges(empty, 'a.py', () => '  é = 1'), [{ line: 0, start: 2, end: 7 }]);
});

const fixed: Plan = {
  id: 'f1', generator: 'use-port',
  files: [
    { path: 'src/a/close_table.py', status: 'changed', text: 'after', lines: 30, added: 3, removed: 1, hunks: [] },
    { path: 'src/a/close_table__test.py', status: 'changed', text: 'after', lines: 9, added: 2, removed: 1, hunks: [] },
    { path: 'src/a/ports/p.py', status: 'new', text: 'new', lines: 4 },
  ],
  holes: [],
  fix: { rule: 'commands-use-ports', file: 'src/a/close_table.py', line: 4, fingerprint: 'x', index: 1, label: 'Use a port the service has' },
};

test('a_changed_file_says_its_added_and_removed_lines', () => {
  assert.deepEqual(fileItems(fixed).map(i => [i.label, i.description]), [
    ['~ src/a/close_table.py', '+3 −1'],
    ['~ src/a/close_table__test.py', '+2 −1'],
    ['+ src/a/ports/p.py', '4 lines'],
  ]);
});

test('a_fix_title_names_the_rule_the_fix_the_counts_and_what_the_finding_settled', () => {
  assert.equal(planTitle(fixed, {}, { app: 'bistro', unit: 'close_table' }),
    'spellbook: fix commands-use-ports · Use a port the service has · 2 files changed · 1 new · 0 holes to fill · app bistro, unit close_table (from the finding)');
  const plain = { ...fixed, files: [fixed.files[0]] };
  assert.equal(planTitle(plain), 'spellbook: fix commands-use-ports · Use a port the service has · 1 file changed · 0 holes to fill');
});

test('changed_files_are_the_ones_the_plan_edits_and_new_files_the_ones_it_writes', () => {
  assert.deepEqual(changedFiles(fixed).map(f => f.path), ['src/a/close_table.py', 'src/a/close_table__test.py']);
  assert.deepEqual(newFiles(fixed).map(f => f.path), ['src/a/ports/p.py']);
});
