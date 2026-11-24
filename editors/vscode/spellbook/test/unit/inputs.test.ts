import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { answerValue, choiceItems, type Generator, nextInput, promptText, retryInput, takenWithoutAsking } from '../../src/inputs';

const command: Generator = {
  id: 'command', name: 'Command', summary: 'A command of a service',
  inputs: [
    { name: 'app', type: 'enum', prompt: 'The app' },
    { name: 'service', type: 'enum', prompt: 'The service', optional: true },
    { name: 'command', type: 'ident', prompt: 'The command' },
  ],
};

test('a_given_value_is_not_asked', () => {
  assert.equal(nextInput(command, {})?.name, 'app');
  assert.equal(nextInput(command, { app: 'bistro' })?.name, 'service');
  assert.equal(nextInput(command, { app: 'bistro', service: 'booking', command: 'x' }), undefined);
});

test('a_skipped_input_is_not_asked_again', () => {
  assert.equal(nextInput(command, { app: 'bistro' }, new Set(['service']))?.name, 'command');
});

test('one_choice_of_a_needed_input_is_taken_without_asking', () => {
  assert.equal(takenWithoutAsking(command.inputs[0], ['bistro']), 'bistro');
  assert.equal(takenWithoutAsking(command.inputs[0], ['a', 'b']), undefined);
});

test('an_optional_input_offers_skip_first_and_is_never_taken_alone', () => {
  assert.deepEqual(choiceItems(command.inputs[1], ['booking']), ['(skip)', 'booking']);
  assert.deepEqual(choiceItems(command.inputs[0], ['bistro']), ['bistro']);
  assert.equal(takenWithoutAsking(command.inputs[1], ['booking']), undefined);
});

test('an_optional_prompt_says_empty_skips', () => {
  assert.equal(promptText(command.inputs[1]), 'The service (empty to skip)');
  assert.equal(promptText(command.inputs[2]), 'The command');
});

test('an_answer_is_trimmed_skip_is_empty_and_a_cancel_stays_a_cancel', () => {
  assert.equal(answerValue('  cancel_table '), 'cancel_table');
  assert.equal(answerValue('(skip)'), '');
  assert.equal(answerValue(undefined), undefined);
});

test('a_plan_stop_naming_an_input_asks_it_once_more_only_once', () => {
  const stop = { stop: 'spellbook: service is not one of …', input: 'service', choices: ['booking'] };
  assert.equal(retryInput(command, stop, false)?.name, 'service');
  assert.equal(retryInput(command, stop, true), undefined);
  assert.equal(retryInput(command, { stop: 'x', input: 'nope' }, false), undefined);
  assert.equal(retryInput(command, { stop: 'x' }, false), undefined);
});
