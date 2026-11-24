// The keys' pure part (no editor code, so node's test runner covers it): which of the server's
// code actions a key runs. keyActions.ts runs it.

// The part of a code action this file reads: its title.
export interface TitledAction {
  title: string;
}

// The four actions a key asks for, by the title the conjure server gives each, and what to say
// when the line offers none of them.
export const KEYS = [
  { command: 'conjure.keys.here', key: 'ctrl+m c', title: 'Conjure here', none: 'conjure: nothing to conjure here' },
  { command: 'conjure.keys.undo', key: 'ctrl+m u', title: 'conjure: Undo', none: 'conjure: nothing to undo on this line' },
  { command: 'conjure.keys.tryHarder', key: 'ctrl+m t', title: 'conjure: Try harder', none: 'conjure: nothing to try harder on this line' },
  { command: 'conjure.keys.showWhy', key: 'ctrl+m w', title: 'conjure: Show why', none: 'conjure: nothing to show on this line' },
] as const;

// actionTitled is the first action with exactly this title, or undefined when the line offers
// none: `actionTitled([{title: 'conjure: Undo'}], 'conjure: Undo')` is that action.
export function actionTitled<T extends TitledAction>(actions: readonly T[], title: string): T | undefined {
  return actions.find(action => action.title === title);
}
