// The keys' pure part (no editor code, so node's test runner covers it): which of the server's
// code actions a key runs. keyActions.ts runs it.

// The part of a code action this file reads: its title.
export interface TitledAction {
  title: string;
}

// One key: the command its chords call, the chords (the same letters as the Neovim plugin's
// `<leader>m` keys, after `ctrl+m`), the title of the server's action it runs and what to say
// when the line offers none. `prefix`: any action whose title starts with `title`
// (an offer carries its price: `lackey: do this · ~$0.50 · sonnet`). `choose`: the matching
// actions are listed to pick from (the four labels) instead of the first one run.
export interface Key {
  command: string;
  keys: readonly string[];
  title: string;
  none: string;
  prefix?: boolean;
  choose?: boolean;
  // the action's name the server knows the key by, so a mark can say which key to press
  // (`ctrl+m o opens the whole answer`)
  name?: string;
}

// keysByName is what the server is told at initialize (`initializationOptions.keys`): each named
// key's first chord by its name.
//
// Example: `{ openAnswer: 'ctrl+m l o' }`.
export function keysByName(keys: readonly Key[]): Record<string, string> {
  return Object.fromEntries(keys.filter(key => key.name).map(key => [key.name, key.keys[0]]));
}

// The keys, the same as the Neovim plugin's `<leader>m` keys, by the titles the lackey server
// gives its actions (crates/lackey/src/adapters/lsp/commands.rs). Do this and stop are right
// under `ctrl+m` and in lackey's own group, `ctrl+m l`.
export const KEYS: readonly Key[] = [
  { command: 'lackey.keys.do', keys: ['ctrl+m a', 'ctrl+m l a'], title: 'lackey: do this', prefix: true, none: 'lackey: no offer on this line: it was not read as an instruction. Ask for something new (`# Add a median(prices) function …`) or label it (ctrl+m l l)' },
  { command: 'lackey.keys.stop', keys: ['ctrl+m q', 'ctrl+m l q'], title: 'lackey: Stop this task', none: 'lackey: no task is running' },
  { command: 'lackey.keys.showDiff', keys: ['ctrl+m l d'], title: 'lackey: Show diff', none: 'lackey: no task changed this line' },
  { command: 'lackey.keys.undoTask', keys: ['ctrl+m l z'], title: 'lackey: Undo this task', none: 'lackey: no task to undo on this line' },
  { command: 'lackey.keys.journal', keys: ['ctrl+m l j'], title: 'lackey: Show journal', none: 'lackey: no task on this line' },
  { command: 'lackey.keys.openAnswer', keys: ['ctrl+m l o'], title: 'lackey: Open the answer', none: 'lackey: no long answer on this line', name: 'openAnswer' },
  { command: 'lackey.keys.dismiss', keys: ['ctrl+m l x'], title: 'lackey: Dismiss', none: 'lackey: nothing to dismiss on this line' },
  { command: 'lackey.keys.label', keys: ['ctrl+m l l'], title: 'lackey: this was', prefix: true, choose: true, none: 'lackey: nothing to label on this line' },
];

// UNDO_LAST is the command of `ctrl+m u` when lackey changed the code last (undoLast.ts).
export const UNDO_LAST = 'lackey.keys.undoLast';

// actionsFor is the actions a key runs or offers, in the server's order: the one titled exactly
// `title`, or, for a prefix key, every action whose title starts with it.
//
// Example: for the label key, the actions `lackey: this was a question`, `… a claim`, `… an
// instruction` and `… nothing` of the line, and none of its `lackey: Dismiss`.
export function actionsFor<T extends TitledAction>(actions: readonly T[], key: Pick<Key, 'title' | 'prefix'>): T[] {
  return actions.filter(action => (key.prefix ? action.title.startsWith(key.title) : action.title === key.title));
}
