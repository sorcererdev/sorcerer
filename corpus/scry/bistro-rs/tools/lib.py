"""Cached `claude -p` runs, fresh copies of the base, and the spend ledger for building bistro-rs.

Every answer is cached under tools/cache/claude by a hash of the request; a rerun without
EXP_LIVE=1 replays it and costs nothing, and an uncached request without EXP_LIVE=1 is an error.
EXP_CEILING (default 4.0, claude-reported USD) stops a live run before it would pass the ceiling."""
import glob, hashlib, json, os, shutil, subprocess, time

HERE = os.path.dirname(os.path.abspath(__file__))
EXP = os.path.dirname(HERE)  # corpus/scry/bistro-rs
BASE = os.path.join(EXP, "base")
CACHE = os.environ.get("EXP_CACHE", os.path.join(HERE, "cache"))
SCRATCH = os.environ.get("EXP_SCRATCH", os.path.join(HERE, "work"))
CEILING = float(os.environ.get("EXP_CEILING", "4.0"))
LIVE = os.environ.get("EXP_LIVE") == "1"
FENCE = ("The repository is the current working directory. Search, read and edit only inside it: never a "
         "parent directory, the home directory or another repository.")


def key(obj):
    return hashlib.sha256(json.dumps(obj, sort_keys=True).encode()).hexdigest()[:20]


def spend():
    return sum(json.load(open(f)).get("cost") or 0 for f in glob.glob(os.path.join(CACHE, "claude", "*.json")))


def sh(args, cwd=None, cap=300, check=True, env=None):
    r = subprocess.run(["perl", "-e", f"alarm {cap}; exec @ARGV", *args], cwd=cwd, capture_output=True, text=True,
                       env=env)
    if check and r.returncode != 0:
        raise RuntimeError(f"{args} failed ({r.returncode}): {(r.stdout + r.stderr)[-1500:]}")
    return r


def clear(d):
    """Removes D, a directory this harness made under the scratch directory, and nothing else."""
    scratch = os.path.realpath(SCRATCH)
    if not os.path.realpath(d).startswith(scratch + os.sep):
        raise RuntimeError(f"refusing to remove {d}: not under {scratch}")
    shutil.rmtree(d)


def base_repo():
    """base/ committed into a git repository under the scratch directory, made once a harness run, so
    copies are clones and patches apply with git."""
    d = os.path.join(SCRATCH, f"base-repo-{os.getpid()}")  # one a process: gen runs side by side
    if os.path.exists(d):
        clear(d)
    os.makedirs(SCRATCH, exist_ok=True)
    sh(["cp", "-R", BASE, d])
    sh(["git", "init", "-q"], cwd=d)
    sh(["git", "add", "-A"], cwd=d)
    sh(["git", "-c", "user.name=corpus", "-c", "user.email=corpus@localhost", "commit", "-qm", "base"], cwd=d)
    return d


_BASE_REPO = None


def fresh_copy(name):
    """A clean clone of the base under the scratch directory."""
    global _BASE_REPO
    if _BASE_REPO is None:
        _BASE_REPO = base_repo()
    d = os.path.join(SCRATCH, name)
    if os.path.exists(d):
        clear(d)
    sh(["git", "clone", "-q", _BASE_REPO, d])
    return d


def _cached(kind, req, call, budget):
    os.makedirs(os.path.join(CACHE, "claude"), exist_ok=True)
    p = os.path.join(CACHE, "claude", kind + "-" + key(req) + ".json")
    if os.path.exists(p):
        return json.load(open(p))
    if not LIVE:
        raise RuntimeError("uncached (set EXP_LIVE=1 to call): " + p)
    s = spend()
    if s + budget > CEILING:
        raise RuntimeError(f"spend ceiling: ${s:.2f} spent, ceiling ${CEILING}")
    rec = call()
    rec["req"] = req
    json.dump(rec, open(p, "w"), indent=1)
    return rec


def _claude(args, prompt, cwd, cap):
    t = time.monotonic()
    r = subprocess.run(["perl", "-e", f"alarm {cap}; exec @ARGV", "env", "-u", "CLAUDECODE", "claude", "-p",
                        "--output-format", "json", "--strict-mcp-config", "--no-session-persistence", *args],
                       cwd=cwd, capture_output=True, input=prompt.encode())
    try:
        j = json.loads(r.stdout)
    except ValueError:
        raise RuntimeError("claude failed: " + (r.stdout + r.stderr).decode()[-800:])
    return j, round(time.monotonic() - t, 1)


def edit_run(prompt, workdir, tag, model="sonnet", cap=400, budget=0.6):
    """claude -p editing a working copy with file tools only. Cached by prompt+model+tag; the cache
    keeps the diff it produced, so a replay re-applies the same patch."""
    def call():
        tools = "Read,Edit,Write,Glob,Grep"
        j, secs = _claude(["--model", model, "--restricted", "--tools", tools, "--allowedTools", tools,
                           "--permission-mode", "acceptEdits", "--append-system-prompt", FENCE,
                           "--max-budget-usd", str(budget)], prompt, workdir, cap)
        sh(["git", "add", "-A"], cwd=workdir)
        diff = sh(["git", "diff", "--cached"], cwd=workdir).stdout
        return {"result": j.get("result", ""), "cost": j.get("total_cost_usd", 0), "secs": secs,
                "turns": j.get("num_turns"), "is_error": j.get("is_error"), "subtype": j.get("subtype"), "diff": diff}
    rec = _cached("edit", {"prompt": prompt, "model": model, "tag": tag}, call, budget)
    return rec


def ask(prompt, tag, model="haiku", system="", cap=180, budget=0.1):
    """claude -p with no tools: one answer to PROMPT, cached by prompt+model+tag+system."""
    def call():
        args = ["--model", model, "--tools", "", "--max-budget-usd", str(budget)]
        if system:
            args += ["--system-prompt", system]
        j, secs = _claude(args, prompt, SCRATCH if os.path.isdir(SCRATCH) else HERE, cap)
        return {"result": j.get("result", ""), "cost": j.get("total_cost_usd", 0), "secs": secs,
                "is_error": j.get("is_error")}
    return _cached("ask", {"prompt": prompt, "model": model, "tag": tag, "system": system}, call, budget)


def json_block(text):
    """The last JSON array or object in TEXT (a model's reply may wrap it in prose or a fence)."""
    for opener, closer in (("[", "]"), ("{", "}")):
        end = text.rfind(closer)
        start = text.rfind(opener, 0, end)
        while start != -1:
            try:
                return json.loads(text[start:end + 1])
            except ValueError:
                start = text.rfind(opener, 0, start)
    raise ValueError("no JSON in reply")
