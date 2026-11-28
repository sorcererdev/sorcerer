import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Generator, Input, Values } from '../../src/inputs';
import { type Asker, newFlow, type Server } from '../../src/newFlow';
import type { Plan } from '../../src/plan';

const command: Generator = {
  id: 'command', name: 'Command', summary: 'A command of a service',
  inputs: [
    { name: 'app', type: 'enum', prompt: 'The app' },
    { name: 'service', type: 'enum', prompt: 'The service' },
    { name: 'command', type: 'ident', prompt: 'The command' },
  ],
};
const plan: Plan = { id: 'p1', generator: 'command', files: [], holes: [] };
const applied = { written: ['a.py'], skipped: [], refused: null, holes: [], first: { file: 'a.py', line: 3 } };

// A server answering from a table of command → reply (a function of the arguments, or a list
// of replies given in turn), recording each call.
function fakeServer(replies: Record<string, unknown>): Server & { calls: [string, unknown[]][] } {
  const calls: [string, unknown[]][] = [];
  const turns: Record<string, number> = {};
  return {
    calls,
    run: async (name, args) => {
      calls.push([name, args]);
      const reply = replies[name];
      if (reply instanceof Error) throw reply;
      if (typeof reply === 'function') return reply(args);
      if (Array.isArray(reply) && name !== 'spellbook.generators') return reply[(turns[name] = (turns[name] ?? -1) + 1)];
      return reply;
    },
  };
}

// An asker giving scripted answers (undefined: a cancel), recording what it was told and asked.
function fakeAsker(answers: { generator?: string; choices?: (string | undefined)[]; texts?: (string | undefined)[]; plan?: 'apply' | 'cancel' }):
  Asker & { said: [string, string][]; asked: string[]; shown: Values[]; settled: (Values | undefined)[] } {
  const said: [string, string][] = [];
  const asked: string[] = [];
  const shown: Values[] = [];
  const settled: (Values | undefined)[] = [];
  const choices = [...(answers.choices ?? [])];
  const texts = [...(answers.texts ?? [])];
  return {
    said, asked, shown, settled,
    pickGenerator: async gs => gs.find(g => g.id === answers.generator),
    pickChoice: async (input: Input, items: string[]) => { asked.push(`${input.name}: ${items.join(',')}`); return choices.shift(); },
    askText: async (input: Input, prompt: string) => { asked.push(`${input.name}? ${prompt}`); return texts.shift(); },
    showPlan: async (_plan, taken, given) => { shown.push(taken); settled.push(given); return answers.plan ?? 'apply'; },
    say: (level, text) => { said.push([level, text]); },
  };
}

function bistro(overrides: Record<string, unknown> = {}) {
  return fakeServer({
    'spellbook.generators': [command],
    'spellbook.choices': (args: unknown[]) => ({ choices: args[2] === 'app' ? ['bistro'] : ['booking', 'web'] }),
    'spellbook.plan': plan,
    'spellbook.apply': applied,
    ...overrides,
  });
}

test('the_full_path_runs_the_four_commands_with_inputs_as_an_object_of_text', async () => {
  const server = bistro();
  const asker = fakeAsker({ generator: 'command', choices: ['booking'], texts: [' cancel_table '] });
  assert.deepEqual(await newFlow(server, asker), applied);
  assert.deepEqual(server.calls, [
    ['spellbook.generators', []],
    ['spellbook.choices', ['command', {}, 'app']],
    ['spellbook.choices', ['command', { app: 'bistro' }, 'service']],
    ['spellbook.plan', ['command', { app: 'bistro', service: 'booking', command: 'cancel_table' }]],
    ['spellbook.apply', ['p1']],
  ]);
  // the one app is taken without asking, and the plan says so
  assert.deepEqual(asker.asked, ['service: booking,web', 'command? The command']);
  assert.deepEqual(asker.shown, [{ app: 'bistro' }]);
  assert.deepEqual(asker.said, []);
});

test('a_given_generator_and_inputs_skip_their_questions', async () => {
  const server = bistro();
  const asker = fakeAsker({ texts: ['x'] });
  await newFlow(server, asker, { generator: 'command', inputs: { service: 'web' } });
  assert.deepEqual(asker.asked, ['command? The command']);
  // a given input was not taken, so only the app is named
  assert.deepEqual(asker.shown, [{ app: 'bistro' }]);
  assert.deepEqual(server.calls.at(-2), ['spellbook.plan', ['command', { service: 'web', app: 'bistro', command: 'x' }]]);
});

test('an_unknown_generator_or_none_at_all_is_said', async () => {
  const asker = fakeAsker({});
  assert.equal(await newFlow(bistro(), asker, { generator: 'nope' }), undefined);
  const none = fakeAsker({});
  assert.equal(await newFlow(bistro({ 'spellbook.generators': [] }), none), undefined);
  assert.deepEqual([...asker.said, ...none.said], [['warn', 'spellbook: no generator named nope'], ['warn', 'spellbook: this project has no generators']]);
});

test('a_plan_stop_naming_an_input_asks_it_once_more_and_a_second_stop_is_said', async () => {
  const stop = { stop: 'spellbook: service must be one of booking', input: 'service', choices: ['booking'] };
  const server = bistro({ 'spellbook.plan': [stop, stop] });
  const asker = fakeAsker({ generator: 'command', choices: ['web', 'booking'], texts: ['x'] });
  assert.equal(await newFlow(server, asker), undefined);
  assert.deepEqual(server.calls.filter(c => c[0] === 'spellbook.plan').map(c => c[1]), [
    ['command', { app: 'bistro', service: 'web', command: 'x' }],
    ['command', { app: 'bistro', service: 'booking', command: 'x' }],
  ]);
  assert.deepEqual(asker.said, [['warn', stop.stop], ['warn', stop.stop]]);
});

test('a_retry_that_then_plans_goes_on_to_apply', async () => {
  const stop = { stop: 'spellbook: which service?', input: 'service', choices: ['booking'] };
  const asker = fakeAsker({ generator: 'command', choices: ['web', 'booking'], texts: ['x'] });
  assert.deepEqual(await newFlow(bistro({ 'spellbook.plan': [stop, plan] }), asker), applied);
});

test('a_list_input_is_asked_as_one_line_of_text_and_a_bad_item_asks_it_again', async () => {
  const listing: Generator = {
    id: 'listing', name: 'Listing', summary: 'A model and its fields',
    inputs: [{ name: 'fields', type: 'list', prompt: 'The fields, name:kind (price:int,notes:text?)' }],
  };
  const stop = { stop: '`fields`: `w:flaot`: `flaot` is not a kind (one of: int, text)', input: 'fields', choices: [] };
  const server = fakeServer({ 'spellbook.generators': [listing], 'spellbook.plan': [stop, plan], 'spellbook.apply': applied });
  const asker = fakeAsker({ generator: 'listing', texts: ['w:flaot', 'w:float'] });
  assert.deepEqual(await newFlow(server, asker), applied);
  assert.deepEqual(asker.asked, [
    'fields? The fields, name:kind (price:int,notes:text?)',
    'fields? The fields, name:kind (price:int,notes:text?)',
  ]);
  assert.deepEqual(server.calls.filter(c => c[0] === 'spellbook.choices'), []);
  assert.deepEqual(asker.said, [['warn', stop.stop]]);
});

test('a_stop_on_any_command_is_said_and_ends', async () => {
  for (const name of ['spellbook.generators', 'spellbook.choices', 'spellbook.plan', 'spellbook.apply']) {
    const asker = fakeAsker({ generator: 'command', choices: ['booking'], texts: ['x'] });
    const reply = { stop: `spellbook: this project is not trusted (${name})` };
    assert.equal(await newFlow(bistro({ [name]: name === 'spellbook.plan' ? [reply] : reply }), asker), undefined, name);
    assert.deepEqual(asker.said, [['warn', reply.stop]], name);
  }
});

test('a_cancel_at_any_step_ends_silently', async () => {
  const cases = [
    fakeAsker({}),
    fakeAsker({ generator: 'command', choices: [undefined] }),
    fakeAsker({ generator: 'command', choices: ['booking'], texts: [undefined] }),
    fakeAsker({ generator: 'command', choices: ['booking'], texts: ['x'], plan: 'cancel' }),
  ];
  for (const asker of cases) {
    const server = bistro();
    assert.equal(await newFlow(server, asker), undefined);
    assert.deepEqual(asker.said, []);
    assert.ok(!server.calls.some(c => c[0] === 'spellbook.apply'));
  }
});

test('a_refused_file_is_said_and_the_reply_still_returned', async () => {
  const refused = { ...applied, refused: { path: 'b.py', why: 'it changed on disk' } };
  const asker = fakeAsker({ generator: 'command', choices: ['booking'], texts: ['x'] });
  assert.deepEqual(await newFlow(bistro({ 'spellbook.apply': refused }), asker), refused);
  assert.deepEqual(asker.said, [['warn', 'spellbook: b.py was not written: it changed on disk']]);
});

test('a_failed_request_is_said_as_an_error', async () => {
  const asker = fakeAsker({});
  assert.equal(await newFlow(bistro({ 'spellbook.generators': new Error('the server stopped') }), asker), undefined);
  assert.deepEqual(asker.said, [['error', 'spellbook: spellbook.generators failed: the server stopped']]);
});

const usePort: Generator = {
  id: 'use-port', name: 'Use a port', summary: 'Use a port the service has',
  inputs: [
    { name: 'app', type: 'enum', prompt: 'The app' },
    { name: 'unit', type: 'ident', prompt: 'The unit' },
    { name: 'port', type: 'enum', prompt: 'The port it takes' },
    { name: 'note', type: 'text', prompt: 'A note', optional: true },
  ],
};
const fix = { file: 'src/a/close_table.py', line: 4, rule: 'commands-use-ports', index: 1, label: 'Use a port the service has', version: 7 };
const fixPlan = { id: 'f1', generator: 'use-port', files: [], holes: [], fix: { rule: fix.rule, file: fix.file, line: 4, fingerprint: 'x', index: 1, label: fix.label } };

function fixing() {
  return bistro({
    'spellbook.generators': [command, usePort],
    'spellbook.choices': () => ({ choices: ['booking_repository', 'table_repository'] }),
    'spellbook.plan': fixPlan,
  });
}

test('a_fix_asks_only_what_is_missing_and_plans_with_the_fix_and_the_version', async () => {
  const server = fixing();
  const asker = fakeAsker({ choices: ['table_repository'] });
  const given = { generator: 'use-port', inputs: { app: 'bistro', unit: 'close_table' }, missing: ['port'], fix };
  assert.deepEqual(await newFlow(server, asker, given), applied);
  assert.deepEqual(asker.asked, ['port: booking_repository,table_repository']);
  const plans = server.calls.filter(c => c[0] === 'spellbook.plan');
  assert.deepEqual(plans, [['spellbook.plan', ['use-port',
    { app: 'bistro', unit: 'close_table', port: 'table_repository' },
    { file: fix.file, line: 4, rule: fix.rule, index: 1, version: 7 }]]]);
  // the inputs the finding settled are named with the plan
  assert.deepEqual(asker.settled, [{ app: 'bistro', unit: 'close_table' }]);
});

test('a_fix_with_nothing_missing_asks_nothing_and_leaves_optional_inputs_out', async () => {
  const asker = fakeAsker({});
  const given = { generator: 'use-port', inputs: { app: 'bistro', unit: 'u', port: 'p' }, missing: [], fix };
  await newFlow(fixing(), asker, given);
  assert.deepEqual(asker.asked, []);
});

test('a_cancelled_fix_question_plans_nothing', async () => {
  const server = fixing();
  const asker = fakeAsker({ choices: [undefined] });
  assert.equal(await newFlow(server, asker, { generator: 'use-port', inputs: {}, missing: ['port'], fix }), undefined);
  assert.ok(!server.calls.some(c => c[0] === 'spellbook.plan'));
});

const syncPlan: Plan = {
  id: 's1', generator: 'sync', holes: [],
  files: [{ path: 'src/booking/commands/mod.rs', status: 'changed', text: 'x', lines: 5, added: 1, region: 'modules' }],
};

test('a_sync_goes_straight_to_the_plan_asking_nothing', async () => {
  const server = fakeServer({ 'spellbook.plan': syncPlan, 'spellbook.apply': { ...applied, changed: ['src/booking/commands/mod.rs'], written: [], first: null } });
  const asker = fakeAsker({});
  const reply = await newFlow(server, asker, { generator: 'sync', inputs: { region: 'modules', file: 'src/booking/commands/mod.rs' }, missing: [] });
  assert.deepEqual(server.calls.map(([name]) => name), ['spellbook.plan', 'spellbook.apply']);
  assert.deepEqual(server.calls[0][1], ['sync', { region: 'modules', file: 'src/booking/commands/mod.rs' }]);
  assert.deepEqual(asker.asked, []);
  assert.deepEqual(reply?.changed, ['src/booking/commands/mod.rs']);
});

test('a_sync_with_nothing_to_do_says_the_regions_are_up_to_date_and_applies_nothing', async () => {
  const server = fakeServer({ 'spellbook.plan': { ...syncPlan, files: [] } });
  const asker = fakeAsker({});
  assert.equal(await newFlow(server, asker, { generator: 'sync', inputs: {} }), undefined);
  assert.deepEqual(asker.said, [['info', 'spellbook: regions up to date']]);
  assert.deepEqual(asker.shown, []);
  assert.deepEqual(server.calls.map(([name]) => name), ['spellbook.plan']);
});
