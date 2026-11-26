import * as vscode from 'vscode';
import { ExecuteCommandRequest } from 'vscode-languageclient/node';
import { folderHere, vscodeAsker } from './asker';
import { endText, stopsInFile } from './holes';
import { startHoleSession } from './holeSession';
import { type ApplyReply, type Asker, type Given, newFlow, type Server, type Stop } from './newFlow';
import { registerFix } from './fixCommand';
import { BEFORE_SCHEME, PLAN_SCHEME, PlanPreviews } from './planPreview';
import type { LanguageClient } from 'vscode-languageclient/node';

// The end-to-end tests answer the questions themselves: what they replace of the real Asker.
export type AskerOverrides = Partial<Asker>;

// registerNew makes Spellbook: New (`spellbook.new`, `ctrl+m n`) and the provider of the plan's
// preview documents. The command takes an optional `{ generator, inputs }` (a keybinding's
// args) that skips those questions. It returns the hook that swaps parts of the Asker.
export function registerNew(context: vscode.ExtensionContext, clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined,
  previews: PlanPreviews): (overrides: AskerOverrides) => void {
  let overrides: AskerOverrides = {};
  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider(PLAN_SCHEME, previews),
    vscode.workspace.registerTextDocumentContentProvider(BEFORE_SCHEME, previews),
    vscode.commands.registerCommand('spellbook.new', (given?: Given) => newHere(clientFor, askerFor(previews, () => overrides), given)),
  );
  registerFix(context, clientFor, askerFor(previews, () => overrides));
  return next => { overrides = next; };
}

// An Asker for a folder: VS Code's own, with what the tests replace of it.
export type AskerFor = (folder: vscode.WorkspaceFolder | undefined) => Asker;

function askerFor(previews: PlanPreviews, overrides: () => AskerOverrides): AskerFor {
  return folder => ({ ...vscodeAsker(previews, folder?.uri), ...overrides() });
}

// newHere runs the New flow against the spellbook server of the folder here, then finishes.
async function newHere(clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined, askerFor: AskerFor, given?: Given): Promise<void> {
  const folder = await folderHere();
  if (!folder) return;
  const asker = askerFor(folder);
  const reply = await flowIn(clientFor, folder, asker, given);
  if (reply) await finish(folder, reply, asker, (written, holes) => endText(written, holes, conjureActive()));
}

// flowIn runs the New flow against the spellbook server of the folder, or says it is not
// running there.
export async function flowIn(clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined,
  folder: vscode.WorkspaceFolder, asker: Asker, given?: Given): Promise<ApplyReply | undefined> {
  const client = clientFor(folder);
  if (!client) {
    asker.say('warn', notRunningText(folder));
    return undefined;
  }
  return newFlow(serverOf(client), asker, given);
}

// serverOf sends the flow's commands to this folder's own client, never through VS Code's
// command registry, so with several folders the right project answers.
function serverOf(client: LanguageClient): Server {
  return { run: (command, args) => client.sendRequest(ExecuteCommandRequest.type, { command, arguments: args }) };
}

// finish ends what New began, after the server has written the files and asked the editor to
// show the first hole:
// 1. with stops, `spellbook.holeSession` on and `spellbook.conjureHoles` off, the first hole's
//    file (once shown) becomes a hole session;
// 2. otherwise, or when the session could not start, the end line is said;
// 3. with `spellbook.conjureHoles` on, conjure fills the first hole.
// The end line is `ended(written, holes)`: a fix says its own.
export async function finish(folder: vscode.WorkspaceFolder, reply: ApplyReply, asker: Asker,
  ended: (written: number, holes: number) => string): Promise<void> {
  const settings = vscode.workspace.getConfiguration('spellbook');
  const conjureHoles = settings.get<boolean>('conjureHoles', false);
  const sessionWanted = settings.get<boolean>('holeSession', true) && !conjureHoles;
  const started = sessionWanted && (reply.stops ?? []).length > 0 && await holeSessionStarted(folder, reply.stops ?? [], reply.changed ?? []);
  if (!started) {
    const holes = reply.holes.filter(hole => hole.source === 'placeholder').length;
    vscode.window.setStatusBarMessage(ended(reply.written.length, holes), 10_000);
  }
  if (reply.first && conjureHoles) await conjureFirst(folder, reply.first, asker);
}

// holeSessionStarted waits up to 2 s for the editor of the first stop's file (the server asks
// VS Code to show it), opens it itself after that, and starts the session there unless the
// document has unsaved changes of its own (a file a fix changed is unsaved by then, and is not
// held back).
async function holeSessionStarted(folder: vscode.WorkspaceFolder, stops: Stop[], changed: string[]): Promise<boolean> {
  const { mine, others } = stopsInFile(stops);
  const uri = vscode.Uri.joinPath(folder.uri, ...mine[0].file.split('/'));
  const shown = await shownEditor(uri, 2000) ?? await openedAt(uri, mine[0]);
  if (!shown || (shown.document.isDirty && !changed.includes(mine[0].file))) return false;
  // the keys go to the focused editor, and the plan's pick had the focus last
  const editor = await vscode.window.showTextDocument(shown.document, { viewColumn: shown.viewColumn, preserveFocus: false });
  return startHoleSession(editor, mine, others, conjureActive());
}

// shownEditor is the visible editor of the URI once there is one, or undefined after `ms`.
async function shownEditor(uri: vscode.Uri, ms: number): Promise<vscode.TextEditor | undefined> {
  const deadline = Date.now() + ms;
  for (;;) {
    // compared as strings: VS Code writes a Windows drive letter in lower case
    const editor = vscode.window.visibleTextEditors.find(e => e.document.uri.toString() === uri.toString());
    if (editor || Date.now() > deadline) return editor;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
}

async function openedAt(uri: vscode.Uri, stop: Stop): Promise<vscode.TextEditor | undefined> {
  const { start, end } = stop.range;
  const selection = new vscode.Range(start.line, start.character, end.line, end.character);
  return Promise.resolve(vscode.window.showTextDocument(uri, { selection })).catch(() => undefined);
}

// conjureFirst runs conjure's `conjure.conjure [uri, line]` on the first hole (line from 1),
// the command conjure's own client registered.
async function conjureFirst(folder: vscode.WorkspaceFolder, first: { file: string; line: number }, asker: Asker): Promise<void> {
  const uri = vscode.Uri.joinPath(folder.uri, ...first.file.split('/'));
  try {
    await vscode.commands.executeCommand('conjure.conjure', uri.toString(), first.line);
  } catch {
    asker.say('warn', 'spellbook: conjure is not running here');
  }
}

// conjureActive says whether the Conjure extension runs in this window.
export function conjureActive(): boolean {
  return vscode.extensions.getExtension('sorcererdev.conjure')?.isActive === true;
}

// `spellbook: not running in bistro; see the Output channel "Spellbook"`
function notRunningText(folder: vscode.WorkspaceFolder): string {
  return `spellbook: not running in ${folder.name}; see the Output channel "Spellbook"`;
}
