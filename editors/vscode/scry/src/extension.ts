import { homedir } from 'node:os';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { LanguageClient, type LanguageClientOptions, type ServerOptions } from 'vscode-languageclient/node';
import { candidates, find, isExecutable } from './binary';

const INSTALL_LINE =
  process.platform === 'win32'
    ? 'powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.ps1 | iex"'
    : "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.sh | sh";

export const NOT_FOUND = `scry was not found. Install it with: ${INSTALL_LINE} (or cargo install sorcerer-scry). Installed already? The installers put it in ~/.local/bin: set scry.path to it, or start VS Code from a shell with ~/.local/bin on PATH.`;

// one `scry lsp` per workspace folder, keyed by the folder's uri
const clients = new Map<string, LanguageClient>();

function startFor(binary: string, folder: vscode.WorkspaceFolder): void {
  const key = folder.uri.toString();
  if (clients.has(key) || folder.uri.scheme !== 'file') return;
  const server: ServerOptions = { command: binary, args: ['lsp'], options: { env: { ...process.env } } };
  const options: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', pattern: `${folder.uri.fsPath}/**` }],
    workspaceFolder: folder,
  };
  const lc = new LanguageClient('scry', 'Scry', server, options);
  clients.set(key, lc);
  lc.start().catch(err => {
    clients.delete(key);
    void vscode.window.showErrorMessage(`scry did not start in ${folder.name}: ${String(err)}`);
  });
}

async function stopFor(folder: vscode.WorkspaceFolder): Promise<void> {
  const key = folder.uri.toString();
  const lc = clients.get(key);
  clients.delete(key);
  if (lc) await lc.stop().catch(() => undefined);
}

export function activate(context: vscode.ExtensionContext): void {
  const setting = vscode.workspace.getConfiguration('scry').get<string>('path', '');
  const version = String(context.extension.packageJSON.version);
  const storage = join(context.globalStorageUri.fsPath, 'bin', version);
  const binary = find(candidates(setting, process.env, homedir(), process.platform, storage), isExecutable);
  if (!binary) {
    void vscode.window.showErrorMessage(NOT_FOUND);
    return;
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) startFor(binary, folder);
  context.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders(e => {
    for (const folder of e.added) startFor(binary, folder);
    for (const folder of e.removed) void stopFor(folder);
  }));
}

export async function deactivate(): Promise<void> {
  const all = [...clients.values()];
  clients.clear();
  await Promise.all(all.map(lc => lc.stop().catch(() => undefined)));
}
