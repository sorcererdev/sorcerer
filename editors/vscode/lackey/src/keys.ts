// The keys' pure part (no editor code, so node's test runner covers it): which of the server's
// code actions a key runs. keyActions.ts runs it.

// The part of a code action this file reads: its title.
export interface TitledAction {
  title: string;
}

// One key: the command its chord calls, the chord, the title of the server's action it runs and
// what to say when the line offers none. `prefix`: any action whose title starts with `title`
// (an offer carries its price: `lackey: do this · ~$0.50 · sonnet`). `choose`: the matching
// actions are listed to pick from (the four labels) instead of the first one run.
export interface Key {
  command: string;
  key: string;
  title: string;
  none: string;
  prefix?: boolean;
  choose?: boolean;
  // the action's name the server knows the key by, so a mark can say which key to press
  // (`ctrl+m o opens the whole answer`)
  name?: string;
}

// keysByName is what the server is told at initialize (`initializationOptions.keys`): each named
// key's chord by its name.
//
// Example: `{ openAnswer: 'ctrl+m o' }`.
export function keysByName(keys: readonly Key[]): Record<string, string> {
  return Object.fromEntries(keys.filter(key => key.name).map(key => [key.name, key.key]));
}

// The eight keys, the same as the Neovim plugin's `<leader>m` keys, by the titles the
// lackey server gives its actions (crates/lackey/src/adapters/lsp/commands.rs).
export const KEYS: readonly Key[] = [
  { command: 'lackey.keys.do', key: 'ctrl+m a', title: 'lackey: do this', prefix: true, none: 'lackey: nothing to do on this line' },
  { command: 'lackey.keys.stop', key: 'ctrl+m q', title: 'lackey: Stop this task', none: 'lackey: no task is running' },
  { command: 'lackey.keys.showDiff', key: 'ctrl+m d', title: 'lackey: Show diff', none: 'lackey: no task changed this line' },
  { command: 'lackey.keys.undoTask', key: 'ctrl+m z', title: 'lackey: Undo this task', none: 'lackey: no task to undo on this line' },
  { command: 'lackey.keys.journal', key: 'ctrl+m j', title: 'lackey: Show journal', none: 'lackey: no task on this line' },
  { command: 'lackey.keys.dismiss', key: 'ctrl+m x', title: 'lackey: Dismiss', none: 'lackey: nothing to dismiss on this line' },
  { command: 'lackey.keys.openAnswer', key: 'ctrl+m o', title: 'lackey: Open the answer', none: 'lackey: no long answer on this line', name: 'openAnswer' },
  { command: 'lackey.keys.label', key: 'ctrl+m l', title: 'lackey: this was', prefix: true, choose: true, none: 'lackey: nothing to label on this line' },
];

// actionsFor is the actions a key runs or offers, in the server's order: the one titled exactly
// `title`, or, for a prefix key, every action whose title starts with it.
//
// Example: for the label key, the actions `lackey: this was a question`, `… a claim`, `… an
// instruction` and `… nothing` of the line, and none of its `lackey: Dismiss`.
export function actionsFor<T extends TitledAction>(actions: readonly T[], key: Pick<Key, 'title' | 'prefix'>): T[] {
  return actions.filter(action => (key.prefix ? action.title.startsWith(key.title) : action.title === key.title));
}
