// The two other notifications apprentice sends only to an editor that handles them, read from
// their params (no editor code, so node's test runner covers it): `sorcerer/showDiff`, the text
// of a file before a task, to diff against the live buffer, and `sorcerer/note`, the agent's
// note to open beside the code. showDiff.ts and extension.ts act on them.

// What *Show diff* sends: the file, its text before the task, and the diff view's title.
export interface ShownDiff {
  uri: string;
  before: string;
  title: string;
}

// The URI scheme of a file's text before a task, the left side of the diff view.
export const BEFORE_SCHEME = 'apprentice-before';

// parseShowDiff reads `{ uri, before, title }`; a missing `before` is an empty text (a file the
// task created) and a missing title is `apprentice: before`. No uri is undefined.
export function parseShowDiff(params: unknown): ShownDiff | undefined {
  const shown = params as Partial<ShownDiff> | null | undefined;
  if (typeof shown?.uri !== 'string') return undefined;
  return {
    uri: shown.uri,
    before: typeof shown.before === 'string' ? shown.before : '',
    title: typeof shown.title === 'string' && shown.title !== '' ? shown.title : 'apprentice: before',
  };
}

// parseNote reads `{ uri }`: the note to open beside the code. No uri is undefined.
export function parseNote(params: unknown): string | undefined {
  const uri = (params as { uri?: unknown } | null | undefined)?.uri;
  return typeof uri === 'string' ? uri : undefined;
}
