import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileItems, type Plan, placeholderRanges, planTitle, planUriPath } from '../../src/plan';

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
