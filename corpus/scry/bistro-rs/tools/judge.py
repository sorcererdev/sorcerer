"""A haiku judge (cached, lib.ask): for every recorded finding of every case, does its statement
name the planted problem (the case's `expect`)? T and N cases have no planted problem: every finding
is judged not a hit without a call. Output tools/results/judged.json:
{case: [{"statement": ..., "hit": bool}]}.
Usage: judge.py CORPUS_DIR [--dry]   (--dry prints the prompts and calls nothing)"""
import glob, json, os, sys, tomllib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I leaves the script's directory out
from lib import HERE, ask, json_block, spend  # noqa: E402

SYSTEM = "You match code review findings against a known answer. Reply with JSON only."

PROMPT = """A code reviewer reviewed a change to a small Rust app (restaurant table bookings) and made the
findings below. One problem was planted in the change on purpose.

The planted problem: {expect}

Findings:
{findings}

For every finding, answer "hit": true if the finding states the planted problem (the same problem in
substance, even in other words); false if it is only near it, or about a different problem in the same
lines.

Reply with a JSON array: [{{"n": <finding number>, "hit": true|false}}, ...]"""


def findings_of(corpus, case):
    """Every recorded finding statement of CASE, over all its packs, in order."""
    p = os.path.join(corpus, "reviews", case + ".json")
    if not os.path.exists(p):
        return []
    out = []
    for pack in json.load(open(p))["packs"]:
        reply = json_block(pack["reply"]) if pack["reply"].strip() else {}
        found = reply if isinstance(reply, list) else reply.get("findings", [])
        out += [f for f in found if f.get("statement")]
    return out


def main():
    corpus, dry = os.path.abspath(sys.argv[1]), "--dry" in sys.argv[2:]
    judged = {}
    for f in sorted(glob.glob(os.path.join(corpus, "cases", "*.toml"))):
        c = tomllib.load(open(f, "rb"))
        fs = findings_of(corpus, c["id"])
        if c["set"] in ("T", "N") or not fs:
            judged[c["id"]] = [{"statement": x["statement"], "hit": False} for x in fs]
            continue
        lines = "\n".join(f"{n + 1}. {x.get('file')}:{x.get('lines')} rule={x.get('rule') or '-'}: {x['statement']}"
                          for n, x in enumerate(fs))
        prompt = PROMPT.format(expect=c["expect"], findings=lines)
        if dry:
            print(f"== {c['id']}\n{prompt}\n")
            continue
        by = {v["n"]: v for v in json_block(ask(prompt, "judge-" + c["id"], system=SYSTEM)["result"])
              if isinstance(v, dict) and "n" in v}
        judged[c["id"]] = [{"statement": x["statement"], "hit": bool(by.get(n + 1, {}).get("hit"))}
                           for n, x in enumerate(fs)]
    if dry:
        return
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    json.dump(judged, open(os.path.join(HERE, "results", "judged.json"), "w"), indent=1)
    hits = sum(x["hit"] for v in judged.values() for x in v)
    print(f"{sum(map(len, judged.values()))} findings judged, {hits} hits; spend ${spend():.2f}")


if __name__ == "__main__":
    main()
