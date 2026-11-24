import * as vscode from 'vscode';
import { ExecuteCommandRequest } from 'vscode-languageclient/node';
import { folderHere, vscodeAsker } from './asker';
import { endText } from './holes';
import { type ApplyReply, type Asker, type Given, newFlow, type Server } from './newFlow';
import { PLAN_SCHEME, PlanPreviews } from './planPreview';
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
    vscode.commands.registerCommand('spellbook.new', (given?: Given) => newHere(clientFor, { ...vscodeAsker(previews), ...overrides }, given)),
  );
  return next => { overrides = next; };
}

// newHere runs the New flow against the spellbook server of the folder here, then finishes.
async function newHere(clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined, asker: Asker, given?: Given): Promise<void> {
  const folder = await folderHere();
  if (!folder) return;
  const client = clientFor(folder);
  if (!client) {
    asker.say('warn', notRunningText(folder));
    return;
  }
  const reply = await newFlow(serverOf(client), asker, given);
  if (reply) await finish(folder, reply, asker);
}

// serverOf sends the flow's commands to this folder's own client, never through VS Code's
// command registry, so with several folders the right project answers.
function serverOf(client: LanguageClient): Server {
  return { run: (command, args) => client.sendRequest(ExecuteCommandRequest.type, { command, arguments: args }) };
}

// finish says the end line, and with `spellbook.conjureHoles` on asks conjure to fill the first
// hole (the server has already opened it).
async function finish(folder: vscode.WorkspaceFolder, reply: ApplyReply, asker: Asker): Promise<void> {
  const holes = reply.holes.filter(hole => hole.source === 'placeholder').length;
  vscode.window.setStatusBarMessage(endText(reply.written.length, holes, conjureActive()), 10_000);
  if (reply.first && vscode.workspace.getConfiguration('spellbook').get<boolean>('conjureHoles', false)) {
    await conjureFirst(folder, reply.first, asker);
  }
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
