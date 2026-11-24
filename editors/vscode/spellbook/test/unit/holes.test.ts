import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { endText } from '../../src/holes';

test('the_end_line_names_conjure_when_it_is_here_else_the_marks', () => {
  assert.equal(endText(2, 3, true), 'spellbook: wrote 2 files · 3 holes · ctrl+m c to conjure the first');
  assert.equal(endText(2, 3, false), 'spellbook: wrote 2 files · 3 holes, marked (Problems)');
  assert.equal(endText(1, 1, false), 'spellbook: wrote 1 file · 1 hole, marked (Problems)');
  assert.equal(endText(2, 0, true), 'spellbook: wrote 2 files');
});

import { doneText, escapeSnippet, messageText, snippetFor, type Stop, stopsInFile } from '../../src/holes';

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
