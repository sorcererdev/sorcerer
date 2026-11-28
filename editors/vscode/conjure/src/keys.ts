// The keys' pure part (no editor code, so node's test runner covers it): which of the server's
// code actions a key runs. keyActions.ts runs it.

// The part of a code action this file reads: its title.
export interface TitledAction {
  title: string;
}

// One key: the command its chords call, the chords (the same letters as the Neovim plugin's
// `<leader>m` keys, after `ctrl+m`), the title of the server's action it runs and what to say
// when the line offers none.
export interface Key {
  command: string;
  keys: readonly string[];
  title: string;
  none: string;
}

// The four keys, by the titles the conjure server gives its actions
// (crates/conjure/src/adapters/lsp/commands.rs). Next step is right under `ctrl+m` and in
// conjure's own group, `ctrl+m c`.
export const KEYS: readonly Key[] = [
  { command: 'conjure.keys.here', keys: ['ctrl+m n', 'ctrl+m c n'], title: 'Conjure here', none: 'conjure: nothing to conjure here' },
  { command: 'conjure.keys.tryHarder', keys: ['ctrl+m c t'], title: 'conjure: Try harder', none: 'conjure: no change on this line to try again' },
  { command: 'conjure.keys.undo', keys: ['ctrl+m c u'], title: 'conjure: Undo', none: 'conjure: no change on this line to undo' },
  { command: 'conjure.keys.showWhy', keys: ['ctrl+m c w'], title: 'conjure: Show why', none: 'conjure: no change on this line' },
];

// UNDO_LAST is the command of `ctrl+m u` when conjure changed the code last (undoLast.ts).
export const UNDO_LAST = 'conjure.keys.undoLast';

// actionTitled is the first action with exactly this title, or undefined when the line offers
// none: `actionTitled([{title: 'conjure: Undo'}], 'conjure: Undo')` is that action.
export function actionTitled<T extends TitledAction>(actions: readonly T[], title: string): T | undefined {
  return actions.find(action => action.title === title);
}
