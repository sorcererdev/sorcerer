"""Measures triage context strategies on the labelled cases with live Jev.

Usage: python3 -I tools/measure.py RUN_DIR [STRATEGY ...]   (from corpus/lackey/triage)
Writes RUN_DIR/<strategy>.json: per case the request's state size, Jev's answers, tokens, time.
Score with tools/score.py. Needs JEV_API_KEY; costs about $0.000042 per 1,000 input tokens.
"""
import difflib, json, os, sys, time, tomllib, urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
MODEL = "jev-1.14.0"
ABOVE, BELOW = 20, 10          # lines of the file around the region
SESSION_CHARS = 12_000         # the session's changes, at most
LATEST_CHARS = 4_000

KIND_TODAY = {
    "question": "it asks something about the code or the project",
    "claim": "it states how the code behaves or should behave",
    "instruction": "it asks for code to be written or changed",
    "aside": "a note to themselves, documentation, or just code",
}
KIND_WORDED = {
    "question": "it asks something about the code, the text or the project, and wants an answer",
    "claim": "it states how the code behaves or should behave, as something to be checked",
    "instruction": "it asks for something to be written or changed: code, tests or text such as docs",
    "aside": "a note for later, documentation for readers, a heading, or just code",
}

KIND_ASKS = {
    "question": "it asks something about the code, the text or the project, and wants an answer",
    "claim": "it states how the code behaves, as something to be checked",
    "instruction": "it asks for something to be done: code, tests or text to be written, filled in or changed",
    "aside": "it is the file's own content (code, documentation, a heading, an explanation) or a note left for later",
}
ASK = ("Is `text` the person asking for something now (an answer, a check, or work done), "
       "rather than content meant to stay in the file for its readers, or a note for later?")

KIND_V4 = {
    "question": "it asks something about the code, the text or the project",
    "claim": "it states how the code behaves, to be checked",
    "instruction": "it asks for something to be written, filled in or changed: code, tests or text",
    "aside": "it is the file's own content: code, documentation, a heading or an explanation",
}
ASK_V4 = ("Does the person want their coding assistant to answer, check or do what `text` says, now?")
ASK_V4_CRITERIA = {"true": "yes: it is a request, a question or a claim to check, for now",
                   "false": "no: it is the file's own content, or a note left for later"}

LATER_V5 = "Is `text` a note left for later or for someone else, rather than something the person wants done or answered now?"
LATER_V5_CRITERIA = {"true": "a note for later, or for someone else", "false": "wanted now"}

KIND_V6 = dict(KIND_V4, instruction=(
    "it asks for something to be written, filled in or changed (code, tests or text), "
    "or it is a placeholder such as TBD standing where content is missing"))


def apply(text, edits, where):
    for find, replace in edits:
        if find not in text:
            sys.exit(f"{where}: anchor not found: {find!r}")
        text = text.replace(find, replace, 1)
    return text


def unified(a, b, name):
    return "".join(difflib.unified_diff(a.splitlines(True), b.splitlines(True), f"a/{name}", f"b/{name}", n=3))


def load_cases():
    cases = tomllib.loads((HERE / "cases.toml").read_text())["case"]
    out = []
    for case in cases:
        sc = HERE / "scenarios" / case["scenario"]
        start = (sc / "files" / case["file"]).read_text()
        before = apply(start, case.get("prior", []), case["id"])
        after = apply(before, case["edit"], case["id"])
        session = (sc / "session.patch").read_text() + unified(start, before, case["file"])
        out.append(dict(case, before=before, after=after, session=session,
                        latest=unified(before, after, case["file"])))
    return out


def region_lines(case):
    lines = case["after"].splitlines()
    want = case["region"].splitlines()
    for i in range(len(lines) - len(want) + 1):
        if [l.rstrip() for l in lines[i:i + len(want)]] == [w.rstrip() for w in want]:
            return i, i + len(want)
    sys.exit(f"{case['id']}: region not in the file")


def where_text(case):
    what = {"comment": "a comment", "prose": "a paragraph", "stub": "an empty body", "hunk": "changed code"}[case["place"]]
    unit = case.get("unit")
    return f"{what} in {case['file']}, in {unit}" if unit else f"{what} in {case['file']}"


def around_text(case):
    lines = case["after"].splitlines()
    s, e = region_lines(case)
    lo, hi = max(0, s - ABOVE), min(len(lines), e + BELOW)
    return "\n".join(("> " if s <= i < e else "  ") + lines[i] for i in range(lo, hi)) + "\n"


def section_text(case):
    """Prose: the heading above the region and whether the section holds anything else."""
    if case["place"] != "prose":
        return None
    lines = case["after"].splitlines()
    s, e = region_lines(case)
    heading = next((i for i in range(s - 1, -1, -1) if lines[i].startswith("#")), None)
    if heading is None:
        return "no heading above it"
    end = next((i for i in range(e, len(lines)) if lines[i].startswith("#")), len(lines))
    others = [l for i, l in enumerate(lines[heading + 1:end], heading + 1) if l.strip() and not s <= i < e]
    if not others:
        return f"`text` is alone under the heading `{lines[heading].strip()}`: the section holds nothing else"
    return f"under the heading `{lines[heading].strip()}`, whose section holds {len(others)} other lines"


def session_text(case):
    """The session's changes: this file's first, then the others, cut at SESSION_CHARS."""
    files = case["session"].split("diff --git ")
    files = ["diff --git " + f for f in files if f.strip()]
    mine = [f for f in files if f" b/{case['file']}" in f.split("\n")[0] or f"+++ b/{case['file']}" in f]
    rest = [f for f in files if f not in mine]
    text = ""
    for f in mine + rest:
        if len(text) + len(f) > SESSION_CHARS:
            text += f"(and {len(mine + rest) - (mine + rest).index(f)} more files changed)\n"
            break
        text += f
    return text + case["latest"][:LATEST_CHARS]


def questions(context_names, wording):
    kinds = {"asks": KIND_ASKS, "v4": KIND_V4, "v5": KIND_V4, "v6": KIND_V6}.get(wording, wording) if isinstance(wording, str) else wording
    q = lambda instr, shape: dict(instructions=instr, **shape)
    noul = lambda instr: q(instr, {"type": "noul", "criteria": {"true": "yes", "false": "no"}})
    return {
        "kind": q("The person wrote or changed `text` in their project. What is it?",
                  {"type": "choice", "criteria": kinds}),
        "addressed": q(LATER_V5, {"type": "noul", "criteria": LATER_V5_CRITERIA}) if wording == "v5" else q(ASK_V4, {"type": "noul", "criteria": ASK_V4_CRITERIA}) if wording in ("v4", "v6") else noul(ASK if wording == "asks" else
                          "Is the text addressed to a reader who could act on it, rather than to no one?"),
        "wants_now": noul("Does the person want an answer now, rather than leaving a note for later?"),
        "needs_decision": noul("Does the text ask for a choice between options?"),
    }


def state_for(case, strategy):
    state = {"text": case["region"], "where": where_text(case)}
    if strategy[0] == "a":
        return state
    state["around"] = around_text(case)
    if (sec := section_text(case)):
        state["section"] = sec
    if strategy[0] == "c":
        state["latest_change"] = case["latest"][:LATEST_CHARS]
    if strategy[0] == "d":
        state["session_changes"] = session_text(case)
    return state


def post(body):
    req = urllib.request.Request("https://api.typesafe.ai/v1/systemone", data=json.dumps(body).encode(),
                                 headers={"content-type": "application/json",
                                          "authorization": "Bearer " + os.environ["JEV_API_KEY"]})
    for attempt in range(4):
        try:
            t = time.time()
            reply = json.load(urllib.request.urlopen(req, timeout=30))
            return reply, time.time() - t
        except Exception as e:  # 429/529: back off
            err = e
            time.sleep(1 + attempt * 2)
    raise err


def candidates(case):
    """Two-stage: the pieces of context Jev may pick from."""
    files = ["diff --git " + f for f in case["session"].split("diff --git ") if f.strip()]
    cands = [("latest change", case["latest"][:LATEST_CHARS])]
    cands += [(f.split("\n")[0][11:], f[:3000]) for f in files][:12]
    return cands


def triage(case, strategy):
    wording = {"2": KIND_WORDED, "3": "asks", "4": "v4", "5": "v5", "6": "v6"}.get(strategy[-1], KIND_TODAY)
    tokens, seconds, stage1 = 0, 0.0, None
    state = state_for(case, strategy)
    if strategy[0] == "e":
        cands = candidates(case)
        st1 = {"text": case["region"], "where": where_text(case), "around": around_text(case)}
        qs = {}
        for n, (name, body) in enumerate(cands):
            st1[f"change_{n}"] = body
            qs[f"relevant_{n}"] = {"type": "noul", "instructions":
                f"Would `change_{n}` ({name}) help tell what `text` is or what it asks for?",
                "criteria": {"true": "yes", "false": "no"}}
        reply, secs = post({"model": MODEL, "state": st1, "questions": qs})
        tokens += reply.get("usage", {}).get("input_tokens", 0); seconds += secs
        picked = [n for n in range(len(cands)) if reply["answers"].get(f"relevant_{n}", {}).get("noul", 0) >= 0.5]
        stage1 = {cands[n][0]: reply["answers"].get(f"relevant_{n}", {}).get("noul") for n in range(len(cands))}
        chosen = "".join(cands[n][1] for n in picked)[:SESSION_CHARS]
        if chosen:
            state["chosen_changes"] = chosen
    reply, secs = post({"model": MODEL, "state": state, "questions": questions(list(state), wording)})
    tokens += reply.get("usage", {}).get("input_tokens", 0); seconds += secs
    return {"id": case["id"], "label": case["label"], "hard": case.get("hard", False),
            "answers": reply["answers"], "input_tokens": tokens, "seconds": seconds,
            "state_chars": sum(len(v) for v in state.values()), "stage1": stage1}


def main():
    run = Path(sys.argv[1]); run.mkdir(parents=True, exist_ok=True)
    strategies = sys.argv[2:] or ["a", "a2", "b", "b2", "c2", "d2", "e2"]
    cases = load_cases()
    for strategy in strategies:
        with ThreadPoolExecutor(4) as pool:
            rows = list(pool.map(lambda c: triage(c, strategy), cases))
        (run / f"{strategy}.json").write_text(json.dumps(rows, indent=1))
        print(strategy, "done", sum(r["input_tokens"] for r in rows), "tokens", flush=True)


if __name__ == "__main__":
    main()
