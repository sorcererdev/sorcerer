// Spellbook: New, as a story over two seams (no editor code, so node's test runner covers it):
// the Server runs spellbook's four commands, the Asker asks the person. extension.ts wires the
// Server to the folder's `spellbook lsp` and asker.ts makes the Asker of VS Code's pickers.
import { answerValue, choiceItems, type Generator, type Input, nextInput, promptText, retryInput, type StopReply, takenWithoutAsking, type Values } from './inputs';
import type { Hole, Plan } from './plan';

// The server: runs one of spellbook's commands and gives its reply (a rejected promise for a
// failed request).
export interface Server {
  run(command: string, args: unknown[]): Promise<unknown>;
}

// The person: picks, types, reads the plan, and is told things.
export interface Asker {
  pickGenerator(generators: Generator[]): Promise<Generator | undefined>;
  pickChoice(input: Input, items: string[]): Promise<string | undefined>;
  askText(input: Input, prompt: string): Promise<string | undefined>;
  showPlan(plan: Plan): Promise<'apply' | 'cancel'>;
  say(level: 'info' | 'warn' | 'error', text: string): void;
}

// One hole of a written file, in the order to walk them: its file under the root (with `/`) and
// its placeholder's range (lines from 0, UTF-16 columns).
export interface Stop {
  file: string;
  name: string;
  kind: string;
  instruction: string;
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
}

// What `spellbook.apply` did: the files written and skipped, a file it refused to write, the
// plan's holes, the first hole (line from 1) and every hole as a stop (absent from an older
// server).
export interface ApplyReply {
  written: string[];
  skipped: string[];
  refused: null | { path: string; why: string };
  holes: Hole[];
  first: null | { file: string; line: number };
  stops?: Stop[];
}

// What the flow was given up front (a keybinding's args, a test): the generator's id and some
// inputs; each skips its question.
export interface Given {
  generator?: string;
  inputs?: Values;
}

// Thrown once the person has been told why the flow ends (a stop, a failed request).
class Ended extends Error {}

// newFlow is Spellbook: New: it returns what apply did, or undefined when the flow ended
// before it (a cancel ends silently; a stop or a failure is said first).
// 1. the generator: the given one, else picked from `spellbook.generators`;
// 2. its inputs, in order, the given ones kept (an enum's choices from `spellbook.choices`);
// 3. the plan (`spellbook.plan`), asking a named input once more if the plan stops for it;
// 4. the plan shown; Apply runs `spellbook.apply` and a refused file is said.
// Example: given { generator: 'command', inputs: { service: 'booking' } } asks only `command`.
export async function newFlow(server: Server, asker: Asker, given: Given = {}): Promise<ApplyReply | undefined> {
  const talk = new Talk(server, asker);
  try {
    const generator = await chosenGenerator(talk, given.generator);
    const values = generator && await answeredInputs(talk, generator, { ...given.inputs });
    const plan = generator && values && await madePlan(talk, generator, values);
    if (!plan || await asker.showPlan(plan) !== 'apply') return undefined;
    return await applied(talk, plan);
  } catch (err) {
    if (err instanceof Ended) return undefined;
    throw err;
  }
}

// The server and the person together: a command whose reply is a stop says it and ends.
class Talk {
  constructor(readonly server: Server, readonly asker: Asker) {}

  // reply runs a command; a `{ stop }` reply is said and ends the flow, unless `keepStop` lets
  // the caller read it.
  async reply<T>(command: string, args: unknown[], keepStop = false): Promise<T> {
    let answer: unknown;
    try {
      answer = await this.server.run(command, args);
    } catch (err) {
      this.end('error', failedText(command, err instanceof Error ? err.message : String(err)));
    }
    if (answer === null || answer === undefined) this.end('error', failedText(command, 'no reply'));
    if (isStop(answer) && !keepStop) this.end('warn', answer.stop);
    return answer as T;
  }

  end(level: 'warn' | 'error', text: string): never {
    this.asker.say(level, text);
    throw new Ended(text);
  }
}

async function chosenGenerator(talk: Talk, named: string | undefined): Promise<Generator | undefined> {
  const generators = await talk.reply<Generator[]>('spellbook.generators', []);
  if (named) return generators.find(g => g.id === named) ?? talk.end('warn', `spellbook: no generator named ${named}`);
  if (generators.length === 0) talk.end('warn', 'spellbook: this project has no generators');
  return talk.asker.pickGenerator(generators);
}

// answeredInputs asks each input with no value yet, in order; undefined when the person
// cancels. A skipped optional input gets no value.
async function answeredInputs(talk: Talk, generator: Generator, values: Values): Promise<Values | undefined> {
  const skipped = new Set<string>();
  for (let input = nextInput(generator, values); input; input = nextInput(generator, values, skipped)) {
    const value = await valueOf(talk, generator, input, values);
    if (value === undefined) return undefined;
    if (value === '') skipped.add(input.name);
    else values[input.name] = value;
  }
  return values;
}

// valueOf asks one input: an enum from its choices (one needed choice taken without asking),
// anything else typed.
async function valueOf(talk: Talk, generator: Generator, input: Input, values: Values): Promise<string | undefined> {
  if (input.type !== 'enum') return asked(talk, input, undefined);
  const { choices } = await talk.reply<{ choices: string[] }>('spellbook.choices', [generator.id, { ...values }, input.name]);
  return takenWithoutAsking(input, choices) ?? asked(talk, input, choices);
}

async function asked(talk: Talk, input: Input, choices: string[] | undefined): Promise<string | undefined> {
  const raw = choices
    ? await talk.asker.pickChoice(input, choiceItems(input, choices))
    : await talk.asker.askText(input, promptText(input));
  return answerValue(raw);
}

// madePlan is the plan; a stop that names an input says why, asks that input once more and
// plans again; a second stop is said and ends.
async function madePlan(talk: Talk, generator: Generator, values: Values, retried = false): Promise<Plan | undefined> {
  const reply = await talk.reply<Plan | StopReply>('spellbook.plan', [generator.id, values], true);
  if (!isStop(reply)) return reply;
  const input = retryInput(generator, reply, retried);
  if (!input) talk.end('warn', reply.stop);
  talk.asker.say('warn', reply.stop);
  const value = await asked(talk, input, reply.choices && reply.choices.length > 0 ? reply.choices : undefined);
  if (!value) return undefined;
  return madePlan(talk, generator, { ...values, [input.name]: value }, true);
}

async function applied(talk: Talk, plan: Plan): Promise<ApplyReply> {
  const reply = await talk.reply<ApplyReply>('spellbook.apply', [plan.id]);
  if (reply.refused) talk.asker.say('warn', refusedText(reply.refused));
  return reply;
}

function isStop(reply: unknown): reply is StopReply {
  return typeof reply === 'object' && reply !== null && typeof (reply as StopReply).stop === 'string';
}

// `spellbook: spellbook.plan failed: the server stopped`
function failedText(command: string, why: string): string {
  return `spellbook: ${command} failed: ${why}`;
}

// `spellbook: src/a.py was not written: it changed on disk`
function refusedText(refused: { path: string; why: string }): string {
  return `spellbook: ${refused.path} was not written: ${refused.why}`;
}
