"""Materialise cases on a fresh copy of the base, apply each patch, and run cargo check, test and
clippy (-D warnings), all offline and locked, with one shared target directory.
Usage: check_case.py [case-id | file.patch ...]   (no arguments: every cases/*.toml)."""
import glob, os, sys, tomllib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I leaves the script's directory out
from lib import EXP, SCRATCH, fresh_copy, sh  # noqa: E402

STEPS = [("check", ["check", "--all-targets"]), ("test", ["test"]),
         ("clippy", ["clippy", "--all-targets", "--", "-D", "warnings"])]


def check(patch):
    """[(step, ok, last line of its output)] for PATCH on a fresh copy; the first failure ends it."""
    d = fresh_copy("check")
    if os.path.getsize(patch):
        sh(["git", "apply", "--whitespace=nowarn", patch], cwd=d)
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(SCRATCH, "target"), CARGO_TERM_COLOR="never")
    out = []
    for name, args in STEPS:
        cmd = ["cargo", args[0], "--offline", "--locked", *args[1:]]
        r = sh(cmd, cwd=d, cap=600, check=False, env=env)
        text = (r.stdout + r.stderr).strip().splitlines()
        errors = [l for l in text if l.startswith(("error", "warning:"))]
        out.append((name, r.returncode == 0, (errors or text or [""])[-1 if r.returncode == 0 else 0]))
        if r.returncode != 0:
            break
    return out


def patch_of(i):
    if i.endswith(".patch"):
        return os.path.abspath(i)
    return os.path.join(EXP, tomllib.load(open(os.path.join(EXP, "cases", i + ".toml"), "rb"))["patch"])


def main():
    ids = sys.argv[1:] or sorted(os.path.basename(f)[:-5] for f in glob.glob(os.path.join(EXP, "cases", "*.toml")))
    bad = 0
    for i in ids:
        if not i.endswith(".patch") and not os.path.getsize(patch_of(i)):
            bad += 1
            print(f"FAIL {i:14} the case's patch is empty", flush=True)
            continue
        steps = check(patch_of(i))
        ok = all(s[1] for s in steps) and len(steps) == len(STEPS)
        bad += not ok
        detail = " ".join(f"{n}={'ok' if good else 'FAIL'}" for n, good, _ in steps)
        print(f"{'ok  ' if ok else 'FAIL'} {os.path.basename(i):14} {detail}" + ("" if ok else f" | {steps[-1][2]}"),
              flush=True)
    print(f"{len(ids) - bad}/{len(ids)} pass")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
