// Which of a generator's inputs to ask, and how, as the Neovim plugin asks them (no editor code,
// so node's test runner covers it); newFlow.ts runs the questions.

// One input of a generator, as `spellbook.generators` sends it.
export interface Input {
  name: string;
  type: 'enum' | 'ident' | 'text' | string;
  prompt: string;
  optional?: boolean;
}

// A generator, as `spellbook.generators` sends it.
export interface Generator {
  id: string;
  name: string;
  summary: string;
  inputs: Input[];
}

// The answers so far, by input name; the server reads them as an object of text.
export type Values = Record<string, string>;

// The item an optional enum offers first, to leave the input out.
export const SKIP = '(skip)';

// nextInput is the first input of the generator that has no value yet, or undefined when every
// input is answered.
// Example: inputs app, service, command with values { app: 'bistro' } give the service input.
export function nextInput(generator: Generator, values: Values, asked: ReadonlySet<string> = new Set()): Input | undefined {
  return generator.inputs.find(input => values[input.name] === undefined && !asked.has(input.name));
}

// choiceItems is what the pick of an enum input lists: its choices, with `(skip)` first when the
// input is optional.
// Example: an optional input with choices a, b lists `(skip)`, a, b.
export function choiceItems(input: Input, choices: readonly string[]): string[] {
  return input.optional ? [SKIP, ...choices] : [...choices];
}

// takenWithoutAsking is the one choice of an input that must be answered and has exactly one
// choice: nothing to ask. Otherwise undefined.
// Example: bistro has one app, so the app input is `bistro` without a pick.
export function takenWithoutAsking(input: Input, choices: readonly string[]): string | undefined {
  return choices.length === 1 && !input.optional ? choices[0] : undefined;
}

// promptText is the question an input box asks: the input's prompt, with `(empty to skip)` when
// the input is optional.
// Example: `The command, a verb phrase (cancel_booking, close_table)`.
export function promptText(input: Input): string {
  return input.optional ? `${input.prompt} (empty to skip)` : input.prompt;
}

// answerValue is the value an answer gives: undefined (a cancel) stays undefined, `(skip)` is the
// empty value, typed text loses its surrounding spaces.
// Example: '  cancel_table ' gives 'cancel_table'.
export function answerValue(raw: string | undefined): string | undefined {
  if (raw === undefined) return undefined;
  if (raw === SKIP) return '';
  return raw.trim();
}

// A plan reply that stopped; `input` names the input it wants, with what it may be.
export interface StopReply {
  stop: string;
  input?: string;
  choices?: string[];
}

// retryInput is the input to ask once more when the plan stopped for a missing or wrong input:
// only on the first retry, and only for an input the generator has. Otherwise undefined, and the
// stop is said.
// Example: a stop naming `service`, not retried yet, gives the service input.
export function retryInput(generator: Generator, stop: StopReply, retried: boolean): Input | undefined {
  if (retried || !stop.input) return undefined;
  return generator.inputs.find(input => input.name === stop.input);
}
