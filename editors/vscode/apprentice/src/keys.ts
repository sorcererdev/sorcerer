// The keys' pure part (no editor code, so node's test runner covers it): which of the server's
// code actions a key runs. keyActions.ts runs it.

// The part of a code action this file reads: its title.
export interface TitledAction {
  title: string;
}

// One key: the command its chord calls, the chord, the title of the server's action it runs and
// what to say when the line offers none. `prefix`: any action whose title starts with `title`
// (an offer carries its price: `apprentice: do this · ~$0.50 · sonnet`). `choose`: the matching
// actions are listed to pick from (the four labels) instead of the first one run.
export interface Key {
  command: string;
  key: string;
  title: string;
  none: string;
  prefix?: boolean;
  choose?: boolean;
}

// The seven keys, the same as the Neovim plugin's `<leader>m` keys, by the titles the
// apprentice server gives its actions (crates/apprentice/src/adapters/lsp/commands.rs).
export const KEYS: readonly Key[] = [
  { command: 'apprentice.keys.do', key: 'ctrl+m a', title: 'apprentice: do this', prefix: true, none: 'apprentice: nothing to do on this line' },
  { command: 'apprentice.keys.stop', key: 'ctrl+m q', title: 'apprentice: Stop this task', none: 'apprentice: no task is running' },
  { command: 'apprentice.keys.showDiff', key: 'ctrl+m d', title: 'apprentice: Show diff', none: 'apprentice: no task changed this line' },
  { command: 'apprentice.keys.undoTask', key: 'ctrl+m z', title: 'apprentice: Undo this task', none: 'apprentice: no task to undo on this line' },
  { command: 'apprentice.keys.journal', key: 'ctrl+m j', title: 'apprentice: Show journal', none: 'apprentice: no task on this line' },
  { command: 'apprentice.keys.dismiss', key: 'ctrl+m x', title: 'apprentice: Dismiss', none: 'apprentice: nothing to dismiss on this line' },
  { command: 'apprentice.keys.label', key: 'ctrl+m l', title: 'apprentice: this was', prefix: true, choose: true, none: 'apprentice: nothing to label on this line' },
];

// actionsFor is the actions a key runs or offers, in the server's order: the one titled exactly
// `title`, or, for a prefix key, every action whose title starts with it.
//
// Example: for the label key, the actions `apprentice: this was a question`, `… a claim`, `… an
// instruction` and `… nothing` of the line, and none of its `apprentice: Dismiss`.
export function actionsFor<T extends TitledAction>(actions: readonly T[], key: Pick<Key, 'title' | 'prefix'>): T[] {
  return actions.filter(action => (key.prefix ? action.title.startsWith(key.title) : action.title === key.title));
}
