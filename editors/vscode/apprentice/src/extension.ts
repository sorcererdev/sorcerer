import { homedir } from 'node:os';
import * as vscode from 'vscode';
import { LanguageClient, type LanguageClientOptions, type ServerOptions, State } from 'vscode-languageclient/node';
import { candidates, find, isExecutable } from './binary';
import { parseNote, parseShowDiff, type ShownDiff } from './diff';
import { assetUrl, binDir, install, releaseBase, releaseSize, targetFor } from './download';
import { type Ghost, parseGhost } from './ghost';
import { Ghosts } from './ghosts';
import { Working } from './inline';
import { registerKeys } from './keyActions';
import { KEYS, keysByName } from './keys';
import { type OfferEnding, remembered } from './offer';
import { BeforeTexts } from './showDiff';
import { registerTaskMarks } from './taskMarks';
import { parseWorking, spelledAs, type WorkingUnit } from './working';

const INSTALL_LINE =
  process.platform === 'win32'
    ? 'powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.ps1 | iex"'
    : "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.sh | sh";

// the sorcerer/… notifications this extension draws; the server sends them only to an editor
// that lists them, and falls back to what every editor shows otherwise
const HANDLED = ['sorcerer/ghost', 'sorcerer/note', 'sorcerer/showDiff'];

// a notification the server ignores: sending it makes the client send the person's edits now,
// not 250 ms later (see startFor)
const SEND_EDITS_NOW = '$/apprentice/sendEditsNow';

// the globalState key holding the version whose offer ended in a way that is remembered (see
// remembered in offer.ts): no offer again until the next version
const ASKED = 'apprentice.declined';
const DOWNLOAD = 'Download';
const MYSELF = "I'll install it myself";

// what each workspace folder's server draws: what it works on, and its ghosts
interface Drawing {
  working: Working;
  ghosts: Ghosts;
}

// one `apprentice lsp` per workspace folder, keyed by the folder's uri
const clients = new Map<string, LanguageClient>();
const drawings = new Map<string, Drawing>();

// What the end-to-end tests read: what is drawn now and since the start.
export interface ApprenticeApi {
  drawn(): WorkingUnit[];
  everDrawn(): WorkingUnit[];
  ghosts(): Ghost[];
  everGhosts(): Ghost[];
  diffs(): ShownDiff[];
}

// startFor starts the folder's server and draws what it sends:
// 1. every file under the folder is the server's (a file a task opens included);
// 2. the person's edits reach it at once: VS Code's client holds a whole-text change back up to
//    250 ms, and a task's write must never land over what was typed a moment ago (the server
//    checks the buffer's version), so each change is followed by a notification that sends it;
// 3. `sorcerer/working`, `sorcerer/ghost`, `sorcerer/note` and `sorcerer/showDiff` are drawn
//    (Working, Ghosts, a note beside, BeforeTexts); everything else (marks, code actions, the
//    picker, edits, progress) is VS Code's own.
function startFor(binary: string, folder: vscode.WorkspaceFolder, before: BeforeTexts): void {
  const key = folder.uri.toString();
  if (clients.has(key) || folder.uri.scheme !== 'file') return;
  const server: ServerOptions = { command: binary, args: ['lsp'], options: { env: { ...process.env } } };
  const options: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', pattern: `${folder.uri.fsPath}/**` }],
    workspaceFolder: folder,
    // the chords by name, so a mark can say which to press (`ctrl+m o opens the whole answer`)
    initializationOptions: { drawsWorking: true, handles: HANDLED, keys: keysByName(KEYS) },
    middleware: {
      didChange: async (event, next) => {
        await next(event);
        if (lc.isRunning()) await lc.sendNotification(SEND_EDITS_NOW).catch(() => undefined);
      },
    },
  };
  const lc: LanguageClient = new LanguageClient('apprentice', 'Apprentice', server, options);
  clients.set(key, lc);
  const drawing: Drawing = { working: new Working(), ghosts: new Ghosts() };
  drawings.set(key, drawing);
  lc.onNotification('sorcerer/working', params =>
    drawing.working.show(spelledAs(parseWorking(params), spelledAsVsCode)));
  lc.onNotification('sorcerer/ghost', params => {
    const ghost = parseGhost(params);
    if (ghost) drawing.ghosts.show({ ...ghost, uri: spelledAsVsCode(ghost.uri) });
  });
  lc.onNotification('sorcerer/note', params => void openBeside(parseNote(params)));
  lc.onNotification('sorcerer/showDiff', params => {
    const diff = parseShowDiff(params);
    if (diff) void before.show(diff);
  });
  // a server that ends mid-work says no more: what it was working on, and its ghosts, go
  lc.onDidChangeState(e => {
    if (e.newState !== State.Stopped) return;
    drawing.working.clear();
    drawing.ghosts.clear();
  });
  lc.start().catch(err => {
    clients.delete(key);
    disposeDrawing(key);
    void vscode.window.showErrorMessage(`apprentice did not start in ${folder.name}: ${String(err)}`);
  });
}

// spelledAsVsCode is a URI the server sent as VS Code spells a document's: the drawings find
// their document by its `uri.toString()`, and on Windows the server's `file:///C:/a.py` is VS
// Code's `file:///c%3A/a.py`.
function spelledAsVsCode(uri: string): string {
  return vscode.Uri.parse(uri).toString();
}

// openBeside opens a note in the column beside the code; the cursor stays where it was. The
// server sends it for a task's note, and for a long answer only when the person asks (ctrl+m o).
async function openBeside(uri: string | undefined): Promise<void> {
  if (!uri) return;
  await vscode.window.showTextDocument(vscode.Uri.parse(uri), { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true, preview: false });
}

function disposeDrawing(key: string): void {
  const drawing = drawings.get(key);
  drawing?.working.dispose();
  drawing?.ghosts.dispose();
  drawings.delete(key);
}

async function stopFor(folder: vscode.WorkspaceFolder): Promise<void> {
  const key = folder.uri.toString();
  const lc = clients.get(key);
  clients.delete(key);
  if (lc) await lc.stop().catch(() => undefined);
  disposeDrawing(key);
}

// activate starts `apprentice lsp` for each workspace folder with the binary it finds (see
// candidates); with none, it offers to download this extension's version of apprentice (no
// release has it yet, so the offer says the install line) and starts once one is in place. The
// offer is not awaited: VS Code need not wait for the user.
export function activate(context: vscode.ExtensionContext): ApprenticeApi {
  registerKeys(context);
  registerTaskMarks(context);
  const before = new BeforeTexts();
  context.subscriptions.push(before);
  const version = String(context.extension.packageJSON.version);
  const storage = context.globalStorageUri.fsPath;
  const setting = vscode.workspace.getConfiguration('apprentice').get<string>('path', '');
  const binary = find(candidates('apprentice', setting, process.env, homedir(), process.platform, binDir(storage, version)), isExecutable);
  if (binary) serve(context, binary, before);
  else void offerDownload(context, version, storage).then(downloaded => downloaded && serve(context, downloaded, before));
  const all = () => [...drawings.values()];
  return {
    drawn: () => all().flatMap(drawing => drawing.working.drawn()),
    everDrawn: () => all().flatMap(drawing => drawing.working.everDrawn()),
    ghosts: () => all().flatMap(drawing => drawing.ghosts.drawn()),
    everGhosts: () => all().flatMap(drawing => drawing.ghosts.everDrawn()),
    diffs: () => before.everShown(),
  };
}

// serve starts a server for every open folder and for each folder added later.
function serve(context: vscode.ExtensionContext, binary: string, before: BeforeTexts): void {
  for (const folder of vscode.workspace.workspaceFolders ?? []) startFor(binary, folder, before);
  context.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders(e => {
    for (const folder of e.added) startFor(binary, folder, before);
    for (const folder of e.removed) void stopFor(folder);
  }));
}

// offerDownload offers to download apprentice of this extension's version, unless an earlier
// ending for this version is remembered (see remembered), and returns the installed binary, or
// undefined when nothing was installed.
async function offerDownload(context: vscode.ExtensionContext, version: string, storage: string): Promise<string | undefined> {
  if (context.globalState.get(ASKED) === version) return undefined;
  const { ending, binary } = await offered(version, storage);
  if (remembered(ending)) await context.globalState.update(ASKED, version);
  return binary;
}

// offered makes the offer and says how it ended:
// 1. a machine the release has no binary for, or a version with no release (a 404, which is
//    every version until apprentice is released), is told the install line instead; a size
//    check that fails otherwise is said;
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
    size = await releaseSize(assetUrl(releaseBase('apprentice', process.env), 'apprentice', version, target));
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
      { location: vscode.ProgressLocation.Notification, title: `Downloading apprentice ${version}` },
      () => install(releaseBase('apprentice', process.env), 'apprentice', version, target, storage),
    );
  } catch (err) {
    void vscode.window.showErrorMessage(`apprentice was not installed: ${errorText(err)}. ${installText()}`);
    return undefined;
  }
}

// `apprentice 0.3.0 is not installed. Download it (12 MB, from github.com/sorcererdev/sorcerer releases)?`
function offerText(version: string, bytes: number): string {
  const size = bytes > 0 ? `${Math.max(1, Math.round(bytes / 1_000_000))} MB, ` : '';
  return `apprentice ${version} is not installed. Download it (${size}from github.com/sorcererdev/sorcerer releases)?`;
}

function installText(): string {
  return `Install apprentice with: ${INSTALL_LINE} (or cargo install sorcerer-apprentice), or set apprentice.path in your user settings, then reload the window.`;
}

function noTargetText(): string {
  return `apprentice has no prebuilt binary for ${process.platform} ${process.arch}; install it with: cargo install sorcerer-apprentice`;
}

function noReleaseText(version: string): string {
  return `apprentice ${version} has no release to download. ${installText()}`;
}

function unreachableText(version: string, err: unknown): string {
  return `apprentice ${version} is not installed, and its release could not be reached (${errorText(err)}). ${installText()}`;
}

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export async function deactivate(): Promise<void> {
  const all = [...clients.values()];
  clients.clear();
  await Promise.all(all.map(lc => lc.stop().catch(() => undefined)));
  for (const key of [...drawings.keys()]) disposeDrawing(key);
}
