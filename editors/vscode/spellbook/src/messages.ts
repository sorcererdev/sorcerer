// Where a message the server shows goes (no editor code, so node's test runner covers it):
// the line a save's sync says goes to the status bar, as a fix's end line does; every other
// message is a notification of its own level, as the language client shows it.

// The protocol's message types: 1 error, 2 warning, 3 info, 4 log.
const ERROR = 1;
const WARNING = 2;

// The start of the line the server says for each region file a save synced.
// Example: `spellbook: synced region modules in src/booking/commands/mod.rs (+1 −0)`.
const SYNCED = 'spellbook: synced region ';

export type MessagePlace = 'status' | 'error' | 'warning' | 'info';

// messagePlace is where a message of TYPE with TEXT is shown.
export function messagePlace(type: number, text: string): MessagePlace {
  if (type === ERROR) return 'error';
  if (type === WARNING) return 'warning';
  return text.startsWith(SYNCED) ? 'status' : 'info';
}
