// The keys' pure part (no editor code, so node's test runner covers it): which of the server's
// code actions a key runs. keyActions.ts runs it; reviewKeys.ts runs the two keys that are no
// code action (review this file now, list findings).

// The part of a code action this file reads: its title.
export interface TitledAction {
  title: string;
}

// One key: the command its chords call, the chords (the same letters as the Neovim plugin's
// `<leader>m` keys, after `ctrl+m`), the title of the server's action it runs and what to say
// when the line offers none. `prefix`: any action whose title starts with `title`; `choose`:
// the matching actions are listed to pick from (here or everywhere) instead of the first one
// run, with `placeHolder` as the question.
export interface Key {
  command: string;
  keys: readonly string[];
  title: string;
  none: string;
  prefix?: boolean;
  choose?: boolean;
  placeHolder?: string;
}

// The keys that run a finding's action, by the titles the scry server gives them
// (crates/scry/src/adapters/lsp/commands.rs). Why is right under `ctrl+m` and in scry's own
// group, `ctrl+m s`.
export const KEYS: readonly Key[] = [
  { command: 'scry.keys.why', keys: ['ctrl+m w', 'ctrl+m s w'], title: 'scry: Show why', none: 'scry: no finding on this line' },
  { command: 'scry.keys.dismiss', keys: ['ctrl+m s x'], title: 'scry: Dismiss', prefix: true, choose: true, placeHolder: 'Dismiss it here, or everywhere?', none: 'scry: no finding on this line' },
  { command: 'scry.keys.showCode', keys: ['ctrl+m s c'], title: 'scry: Show code', none: 'scry: no finding on this line cites code' },
];

// The two keys that are no code action.
export const REVIEW = { command: 'scry.keys.review', keys: ['ctrl+m s r'] } as const;
export const LIST = { command: 'scry.keys.list', keys: ['ctrl+m s l'] } as const;

// actionsFor is the actions a key runs or offers, in the server's order: the one titled exactly
// `title`, or, for a prefix key, every action whose title starts with it.
//
// Example: for the dismiss key, the line's `scry: Dismiss here` and `scry: Dismiss everywhere`,
// and not its `scry: Show why`.
export function actionsFor<T extends TitledAction>(actions: readonly T[], key: Pick<Key, 'title' | 'prefix'>): T[] {
  return actions.filter(action => (key.prefix ? action.title.startsWith(key.title) : action.title === key.title));
}
