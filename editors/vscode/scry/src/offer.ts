// How an offer to download scry ended. extension.ts makes the offer; this says which endings
// it remembers for the extension's version, so that the offer is not made again until the
// next version.
export type OfferEnding =
  | 'no target' // the release has no binary for this machine
  | 'no release' // the release answered 404: this version was never released
  | 'unreachable' // offline, or the release answered with an error
  | 'dismissed' // the offer was closed without an answer
  | 'installing it myself' // the user chose "I'll install it myself"
  | 'download failed' // a cut-off download, or a sum that did not match
  | 'downloaded';

// remembered says whether an ending keeps the offer quiet for the rest of this version: only
// the user's own "I'll install it myself", and the two answers that cannot change before the
// next version (no binary for this machine, no release). A failed download, a mismatch, an
// unreachable release or a dismissed offer is offered again on the next start (user ruling,
// 2026-11-22: retrying is safe because nothing unchecked is ever run).
export function remembered(ending: OfferEnding): boolean {
  return ending === 'installing it myself' || ending === 'no release' || ending === 'no target';
}
