# bistro-rs tools: the generation harness

Scripts that built the bistro-rs cases and labels. Run them from this directory (or by path from the
repository) with `python3 -I`; each needs only Python 3.11+, git, cargo and, for live runs, `claude`.

| script | what it does |
|---|---|
| `lib.py` | cached `claude -p` runs (`edit_run`: file tools only, `--restricted`, the fence, `--max-budget-usd`; `ask`: no tools), fresh copies of the base (a git clone of `base/` committed under `work/base-repo/`), the spend ledger, `sh` under `perl -e 'alarm N'` |
| `gen.py` | the candidates: `--list` prints every prompt; `gen.py ID…` runs or replays each and saves `gen/ID.patch` and `gen/ID.json`; `--fix ID` makes one follow-up run with the compiler's output |
| `check_case.py` | each case (or patch) on a fresh copy: `cargo check`, `cargo test`, `cargo clippy -D warnings`, offline and locked, one shared target directory; `N/M pass`, exit 1 on any failure |
| `derive.py` | a scripted hand edit on top of a patch (`sub`, `write`, `drop_test`), saved as a patch in `gen/` |
| `judge.py` | haiku marks each recorded finding hit or not against the case's `expect` → `results/judged.json` (`--dry` prints the prompts) |
| `label.py` | `labels.json` from `results/judged.json` and `results/overrides.json` (the user's corrections win) |
| `test_lib.py` | the harness's own checks, offline |

**Replay is free.** Every model answer is cached under `cache/claude/` by a hash of its request;
any script run without `EXP_LIVE=1` replays, and a request with no cached answer is an error, never a
call. **The ceiling:** `EXP_CEILING` (default `4.0`, claude-reported USD; on a subscription it is usage,
not dollars) stops a live run before it would pass it. **The stop rule:** a candidate that fails its
checks gets one `--fix` run, then a scripted hand fix with `derive.py`; nothing is edited by hand
outside a script.

`cache/` and `work/` are not in git (the corpus's `.gitignore`); `gen/` (every patch and prompt record)
and `results/` are.
