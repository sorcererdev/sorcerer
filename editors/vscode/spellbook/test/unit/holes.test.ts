import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { doneText, endText, escapeSnippet, messageText, type Range, shifted, snippetFor, type Stop, stopAt, stopsInFile } from '../../src/holes';

test('the_end_line_names_conjure_when_it_is_here_else_the_marks', () => {
  assert.equal(endText(2, 3, true), 'spellbook: wrote 2 files · 3 holes · ctrl+m c to conjure the first');
  assert.equal(endText(2, 3, false), 'spellbook: wrote 2 files · 3 holes, marked (Problems)');
  assert.equal(endText(1, 1, false), 'spellbook: wrote 1 file · 1 hole, marked (Problems)');
  assert.equal(endText(2, 0, true), 'spellbook: wrote 2 files');
});

function stop(name: string, line: number, start: number, end: number, file = 'src/c/cancel_table.py', endLine = line): Stop {
  return { file, name, kind: 'ident', instruction: `its ${name}`, range: { start: { line, character: start }, end: { line: endLine, character: end } } };
}

const FILE = [
  'class CancelTableInput:',
  '    todo_field: object',
  '',
  '    def handle(self) -> int:',
  '        raise NotImplementedError("hole handle: $1 {x}")',
];

test('two_stops_on_one_line_become_two_placeholders_and_the_last_is_followed_by_0', () => {
  const stops = [stop('field', 1, 4, 14), stop('type', 1, 16, 22)];
  assert.deepEqual(snippetFor(FILE, stops), { region: { startLine: 1, endLine: 1 }, snippet: '    ${1:todo_field}: ${2:object}$0' });
});

test('a_stop_on_a_later_line_spans_the_lines_between_escaped', () => {
  const stops = [stop('field', 1, 4, 14), stop('handle', 4, 8, FILE[4].length)];
  const { region, snippet } = snippetFor(FILE, stops);
  assert.deepEqual(region, { startLine: 1, endLine: 4 });
  assert.equal(snippet, [
    '    ${1:todo_field}: object',
    '',
    '    def handle(self) -> int:',
    '        ${2:raise NotImplementedError("hole handle: \\$1 {x\\}")}$0',
  ].join('\n'));
});

test('dollar_brace_and_backslash_are_literal', () => {
  assert.equal(escapeSnippet('a\\b $c }'), 'a\\\\b \\$c \\}');
});

test('a_whole_line_stop_is_the_text_after_the_indent', () => {
  const { snippet } = snippetFor(FILE, [stop('handle', 4, 0, 0, undefined, 5)]);
  assert.equal(snippet, '        ${1:raise NotImplementedError("hole handle: \\$1 {x\\}")}$0');
});

test('columns_are_utf16_so_text_before_a_stop_may_be_non_ascii', () => {
  const lines = ['    é🙂 = todo'];
  // 'é' is one UTF-16 unit, '🙂' two: todo starts at column 4 + 1 + 2 + 3 = 10
  assert.equal(snippetFor(lines, [stop('value', 0, 10, 14)]).snippet, '    é🙂 = ${1:todo}$0');
});

test('the_session_runs_in_the_first_stops_file_and_counts_the_others', () => {
  const stops = [stop('field', 1, 4, 14), stop('field', 10, 0, 5, 'src/c/cancel_table__test.py'), stop('type', 1, 16, 22), stop('x', 2, 0, 1, 'src/d/b.py')];
  const { mine, others } = stopsInFile(stops);
  assert.deepEqual(mine.map(s => s.name), ['field', 'type']);
  assert.deepEqual([...others], [['cancel_table__test.py', 1], ['b.py', 1]]);
  assert.deepEqual(stopsInFile([]), { mine: [], others: new Map() });
});

test('the_message_says_where_you_are_and_the_last_hole_says_done', () => {
  const s = stop('field', 1, 4, 14);
  assert.equal(messageText(1, 3, s, true), 'hole 1 of 3 · field: its field · type to replace · Tab next · ctrl+m c conjure');
  assert.equal(messageText(2, 3, s, false), 'hole 2 of 3 · field: its field · type to replace · Tab next');
  assert.equal(messageText(3, 3, s, false), 'hole 3 of 3 · field: its field · type to replace · Tab done');
});

test('done_names_the_other_files_holes', () => {
  assert.equal(doneText(new Map()), 'spellbook: holes done');
  assert.equal(doneText(new Map([['cancel_table__test.py', 1]])), 'spellbook: holes done · 1 more in cancel_table__test.py (Problems)');
  assert.equal(doneText(new Map([['b.py', 1], ['a.py', 1]])), 'spellbook: holes done · 2 more in a.py, b.py (Problems)');
});

const at = (line: number, character: number) => ({ line, character });
const r = (l1: number, c1: number, l2: number, c2: number): Range => ({ start: at(l1, c1), end: at(l2, c2) });
const edit = (range: Range, text: string) => ({ range, text });
const FIELD = r(1, 4, 1, 14);

test('an_insert_before_a_range_on_its_line_moves_it', () => {
  assert.deepEqual(shifted([FIELD], [edit(r(1, 0, 1, 0), 'ab')]), [r(1, 6, 1, 16)]);
});

test('typing_over_the_whole_range_then_at_its_end_grows_it', () => {
  const typed = shifted([FIELD], [edit(FIELD, 'i')]);
  assert.deepEqual(typed, [r(1, 4, 1, 5)]);
  assert.deepEqual(shifted(typed, [edit(r(1, 5, 1, 5), 'd')]), [r(1, 4, 1, 6)]);
});

test('typing_at_the_start_or_inside_grows_it', () => {
  assert.deepEqual(shifted([FIELD], [edit(r(1, 4, 1, 4), 'x')]), [r(1, 4, 1, 15)]);
  assert.deepEqual(shifted([FIELD], [edit(r(1, 6, 1, 8), '')]), [r(1, 4, 1, 12)]);
});

test('an_edit_after_a_range_leaves_it', () => {
  assert.deepEqual(shifted([FIELD], [edit(r(1, 16, 1, 22), 'int')]), [FIELD]);
  assert.deepEqual(shifted([FIELD], [edit(r(3, 0, 3, 0), 'x\ny\n')]), [FIELD]);
});

test('a_delete_across_a_ranges_start_cuts_it_there', () => {
  assert.deepEqual(shifted([FIELD], [edit(r(1, 2, 1, 6), '')]), [r(1, 2, 1, 10)]);
});

test('a_multi_line_paste_before_moves_later_lines_and_the_same_lines_columns', () => {
  const later = r(4, 8, 4, 20);
  assert.deepEqual(shifted([FIELD, later], [edit(r(1, 0, 1, 0), 'a\nbc')]), [r(2, 6, 2, 16), r(5, 8, 5, 20)]);
  assert.deepEqual(shifted([later], [edit(r(1, 0, 2, 0), '')]), [r(3, 8, 3, 20)]);
});

test('edits_apply_in_order', () => {
  assert.deepEqual(shifted([FIELD], [edit(r(1, 0, 1, 0), 'a'), edit(r(1, 0, 1, 0), 'b')]), [r(1, 6, 1, 16)]);
});

test('a_position_is_in_a_range_at_both_ends', () => {
  const ranges = [FIELD, r(1, 16, 1, 22)];
  assert.equal(stopAt(ranges, at(1, 4)), 0);
  assert.equal(stopAt(ranges, at(1, 14)), 0);
  assert.equal(stopAt(ranges, at(1, 22)), 1);
  assert.equal(stopAt(ranges, at(1, 15)), -1);
  assert.equal(stopAt(ranges, at(0, 0)), -1);
});
