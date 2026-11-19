# The scry corpus

Inspect's 1b replay gate, ported: 58 bistro cases (18 V-must, 8 B, 18 T, 8 N, 6 V-taste) and
the recordings `crates/scry/tests/gate.rs` replays. Written by `scripts/port-scry-corpus`;
the notes directory is not under git, so each source is pinned by its sha256.

| path | from |
|---|---|
| `base/` | `git archive HEAD` of `~/projects/notes/durandal/experiments/code-checker-conventions-2026-11-14/base` at `42d1a84f4d368b16d5c56e671f0b2360610749bd`, `.olifant/` dropped |
| `cases/` | `~/projects/notes/durandal/experiments/code-checker-conventions-2026-11-14/cases`, the 58 labelled cases (the borderlines bl01-bl03 left out) |
| `steering.md` | `~/projects/notes/durandal/experiments/vet-1a-2026-11-15/vet.md` (sha256 `0354e968f02a137c5109b93f99a395e698002b14bf334a9d4256db40b3b50c83`) |
| `labels.json` | `~/projects/olifant/src/Olifant/Inspect/testdata/inspect-corpus-labels.json` (sha256 `8ee71284f2c049f80fb0ade019372814ae9653b02816afce99646243c6e95b6f`), `patch` dropped |
| `finds/<case>.json` | `~/projects/olifant/src/Olifant/Inspect/testdata/inspect-corpus-finds.json` (sha256 `0cdc587768b87c877d0eee8ad3ff9262dfb5facb5df8bc0ee9f85f585e154b7b`): 1a's sonnet replies, one per changed file, merged into one reply per case |
| `checks.json` | `~/projects/olifant/src/Olifant/Inspect/testdata/inspect-corpus-checks.json` (sha256 `07806ba4157d9343fe393364b5b0d47d15124ba497f62c995aac48059777b4ca`): 1a's claim-check verdicts by statement |
| `jev/<key>.json` | Jev's answers, recorded by the gate itself (`SCRY_RECORD_JEV=1`), one file per question |

Paths are on the machine that ran the port; `~/projects/notes/durandal/experiments` and
`~/projects/olifant/src/Olifant/Inspect/testdata`.
