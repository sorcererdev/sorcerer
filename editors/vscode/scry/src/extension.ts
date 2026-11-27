import { homedir } from 'node:os';
import * as vscode from 'vscode';
import { LanguageClient, type LanguageClientOptions, type ServerOptions, State } from 'vscode-languageclient/node';
import { candidates, find, isExecutable } from './binary';
import { assetUrl, binDir, install, releaseBase, releaseSize, targetFor } from './download';
import { Working } from './inline';
import { type OfferEnding, remembered } from './offer';
import { parseWorking, spelledAs, type WorkingUnit } from './working';

const INSTALL_LINE =
  process.platform === 'win32'
    ? 'powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.ps1 | iex"'
    : "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.sh | sh";

// the globalState key holding the version whose offer ended in a way that is remembered (see
// remembered in offer.ts): no offer again until the next version
const ASKED = 'scry.declined';
const DOWNLOAD = 'Download';
const MYSELF = "I'll install it myself";

// one `scry lsp` per workspace folder, keyed by the folder's uri
const clients = new Map<string, LanguageClient>();
// what each server is working on, drawn after the units' first lines
const workings = new Map<string, Working>();

function startFor(binary: string, folder: vscode.WorkspaceFolder): void {
  const key = folder.uri.toString();
  if (clients.has(key) || folder.uri.scheme !== 'file') return;
  const server: ServerOptions = { command: binary, args: ['lsp'], options: { env: { ...process.env } } };
  const options: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', pattern: `${folder.uri.fsPath}/**` }],
    workspaceFolder: folder,
    // tells the server this editor draws `sorcerer/working` (see Working)
    initializationOptions: { drawsWorking: true },
  };
  const lc = new LanguageClient('scry', 'Scry', server, options);
  clients.set(key, lc);
  const working = new Working();
  workings.set(key, working);
  lc.onNotification('sorcerer/working', params => working.show(spelledAs(parseWorking(params), spelledAsVsCode)));
  // a server that ends mid-work says no more: what it was working on is taken away
  lc.onDidChangeState(e => e.newState === State.Stopped && working.clear());
  lc.start().catch(err => {
    clients.delete(key);
    workings.get(key)?.dispose();
    workings.delete(key);
    void vscode.window.showErrorMessage(`scry did not start in ${folder.name}: ${String(err)}`);
  });
}

async function stopFor(folder: vscode.WorkspaceFolder): Promise<void> {
  const key = folder.uri.toString();
  const lc = clients.get(key);
  clients.delete(key);
  if (lc) await lc.stop().catch(() => undefined);
  workings.get(key)?.dispose();
  workings.delete(key);
}

// activate starts `scry lsp` for each workspace folder with the binary it finds (see
// candidates); with none, it offers to download this extension's version of scry and starts
// once that is in place. The offer is not awaited: VS Code need not wait for the user.
export function activate(context: vscode.ExtensionContext): { drawn(): WorkingUnit[]; everDrawn(): WorkingUnit[] } {
  const version = String(context.extension.packageJSON.version);
  const storage = context.globalStorageUri.fsPath;
  const setting = vscode.workspace.getConfiguration('scry').get<string>('path', '');
  const binary = find(candidates('scry', setting, process.env, homedir(), process.platform, binDir(storage, version)), isExecutable);
  if (binary) serve(context, binary);
  else void offerDownload(context, version, storage).then(downloaded => downloaded && serve(context, downloaded));
  // for the end-to-end tests: what is drawn now
  return {
    drawn: () => [...workings.values()].flatMap(working => working.drawn()),
    everDrawn: () => [...workings.values()].flatMap(working => working.everDrawn()),
  };
}

// serve starts a server for every open folder and for each folder added later.
function serve(context: vscode.ExtensionContext, binary: string): void {
  for (const folder of vscode.workspace.workspaceFolders ?? []) startFor(binary, folder);
  context.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders(e => {
    for (const folder of e.added) startFor(binary, folder);
    for (const folder of e.removed) void stopFor(folder);
  }));
}

// offerDownload offers to download scry of this extension's version, unless an earlier
// ending for this version is remembered (see remembered), and returns the installed binary, or
// undefined when nothing was installed.
async function offerDownload(context: vscode.ExtensionContext, version: string, storage: string): Promise<string | undefined> {
  if (context.globalState.get(ASKED) === version) return undefined;
  const { ending, binary } = await offered(version, storage);
  if (remembered(ending)) await context.globalState.update(ASKED, version);
  return binary;
}

// offered makes the offer and says how it ended:
// 1. a machine the release has no binary for, or a version with no release (a 404), is told
//    the install line instead; a size check that fails otherwise is said;
// 2. otherwise the offer, with the archive's size; Download installs it, checked against its
//    sum; "I'll install it myself" shows the install line.
async function offered(version: string, storage: string): Promise<{ ending: OfferEnding; binary?: string }> {
  const target = targetFor(process.platform, process.arch);
  if (!target) {
    void vscode.window.showWarningMessage(noTargetText());
    return { ending: 'no target' };
  }
  let size: number | undefined;
  try {
    size = await releaseSize(assetUrl(releaseBase('scry', process.env), 'scry', version, target));
  } catch (err) {
    void vscode.window.showWarningMessage(unreachableText(version, err));
    return { ending: 'unreachable' };
  }
  if (size === undefined) {
    void vscode.window.showWarningMessage(noReleaseText(version));
    return { ending: 'no release' };
  }
  const answer = await vscode.window.showInformationMessage(offerText(version, size), DOWNLOAD, MYSELF);
  if (answer === MYSELF) {
    void vscode.window.showInformationMessage(installText());
    return { ending: 'installing it myself' };
  }
  if (answer !== DOWNLOAD) return { ending: 'dismissed' };
  const binary = await downloaded(version, target, storage);
  return binary ? { ending: 'downloaded', binary } : { ending: 'download failed' };
}

// downloaded installs the binary under a progress notification; a failure (a sum that does
// not match, a cut-off download) is said with the install line, nothing runs, and the offer
// comes again on the next start.
async function downloaded(version: string, target: string, storage: string): Promise<string | undefined> {
  try {
    return await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Downloading scry ${version}` },
      () => install(releaseBase('scry', process.env), 'scry', version, target, storage),
    );
  } catch (err) {
    void vscode.window.showErrorMessage(`scry was not installed: ${errorText(err)}. ${installText()}`);
    return undefined;
  }
}

// `scry 0.1.0 is not installed. Download it (12 MB, from github.com/sorcererdev/sorcerer releases)?`
function offerText(version: string, bytes: number): string {
  const size = bytes > 0 ? `${Math.max(1, Math.round(bytes / 1_000_000))} MB, ` : '';
  return `scry ${version} is not installed. Download it (${size}from github.com/sorcererdev/sorcerer releases)?`;
}

function installText(): string {
  return `Install scry with: ${INSTALL_LINE} (or cargo install sorcerer-scry), then reload the window.`;
}

function noTargetText(): string {
  return `scry has no prebuilt binary for ${process.platform} ${process.arch}; install it with: cargo install sorcerer-scry`;
}

function noReleaseText(version: string): string {
  return `scry ${version} has no release to download. ${installText()}`;
}

function unreachableText(version: string, err: unknown): string {
  return `scry ${version} is not installed, and its release could not be reached (${errorText(err)}). ${installText()}`;
}

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export async function deactivate(): Promise<void> {
  const all = [...clients.values()];
  clients.clear();
  await Promise.all(all.map(lc => lc.stop().catch(() => undefined)));
  for (const working of workings.values()) working.dispose();
  workings.clear();
}

// spelledAsVsCode is a URI the server sent as VS Code spells a document's: the drawing finds its
// document by `uri.toString()`, and on Windows the server's `file:///C:/a.py` is VS Code's
// `file:///c%3A/a.py`.
function spelledAsVsCode(uri: string): string {
  return vscode.Uri.parse(uri).toString();
}
