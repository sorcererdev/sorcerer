"""A scripted hand edit, saved as a patch, so every hand change to a model's patch is a file in gen/.

Usage: derive.py NAME BASE.patch OUT.patch [DROP_PATH ...] < edits.py
Applies BASE.patch (empty: the base itself) to a fresh copy, restores every changed file whose path
contains a DROP_PATH substring to the base (a new file is removed), runs the Python edits read from
stdin with `src` (the copy's src/) and the helpers sub(path, old, new), write(path, text) and
drop_test(path, name), then saves the copy's diff against the base as OUT.patch."""
import os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I leaves the script's directory out
from lib import fresh_copy, sh  # noqa: E402

name, base, out, *drop = sys.argv[1:]
d = fresh_copy("derive-" + name)
if os.path.getsize(base):
    sh(["git", "apply", "--whitespace=nowarn", os.path.abspath(base)], cwd=d)
if drop:
    sh(["git", "add", "-A"], cwd=d)
    for f in sh(["git", "diff", "--cached", "--name-only"], cwd=d).stdout.split():
        if any(s in f for s in drop):
            status = sh(["git", "diff", "--cached", "--name-status", "--", f], cwd=d).stdout.split()[0]
            if status == "A":
                sh(["git", "rm", "-q", "-f", f], cwd=d)
            else:
                sh(["git", "checkout", "-q", "HEAD", "--", f], cwd=d)
    sh(["git", "reset", "-q"], cwd=d)
src = os.path.join(d, "src")


def sub(path, old, new, count=1):
    p = os.path.join(src, path)
    s = open(p).read()
    if old not in s:
        raise SystemExit(f"not found in {path}: {old!r}")
    open(p, "w").write(s.replace(old, new, count))


def write(path, text):
    p = os.path.join(src, path)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "w").write(text)


def drop_test(path, name):
    """Remove `#[test] fn NAME` (with its attribute and the blank line before it) from PATH."""
    p = os.path.join(src, path)
    s = open(p).read()
    m = re.search(r"\n\n(    #\[test\]\n)?    fn " + re.escape(name) + r"\(.*?\n    }\n", s, flags=re.S)
    if not m:
        raise SystemExit("no test " + name)
    open(p, "w").write(s[:m.start()] + "\n" + s[m.end():])


exec(sys.stdin.read())
sh(["git", "add", "-A"], cwd=d)
open(out, "w").write(sh(["git", "diff", "--cached"], cwd=d).stdout)
sh(["git", "reset", "-q"], cwd=d)
print("saved", out)
