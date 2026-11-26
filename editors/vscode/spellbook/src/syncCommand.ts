import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { folderHere } from './asker';
import { finish, flowIn, type AskerFor } from './newCommand';
import { SYNC, syncedText } from './plan';

// registerSync makes Spellbook: Sync (`spellbook.sync`, `ctrl+m s`): every owned region
// refreshed, the plan shown as a fix's is (the diff of each region file), Apply lands it through
// the editor. It is Spellbook: New entered at the plan with the generator `sync`.
export function registerSync(context: vscode.ExtensionContext, clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined,
  askerFor: AskerFor): void {
  context.subscriptions.push(vscode.commands.registerCommand('spellbook.sync', () => syncHere(clientFor, askerFor)));
}

// syncHere plans the sync of the folder here; nothing to do is said (`spellbook: regions up to
// date`); otherwise, once applied, the status line says how many files it changed.
async function syncHere(clientFor: (folder: vscode.WorkspaceFolder) => LanguageClient | undefined, askerFor: AskerFor): Promise<void> {
  const folder = await folderHere();
  if (!folder) return;
  const asker = askerFor(folder);
  const reply = await flowIn(clientFor, folder, asker, { generator: SYNC, inputs: {} });
  if (reply) await finish(folder, reply, asker, () => syncedText((reply.changed ?? []).length));
}
