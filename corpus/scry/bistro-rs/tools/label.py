"""The corpus's labels.json from the judge's marks and the user's overrides, printed to stdout.

Usage: label.py CORPUS_DIR JUDGED.json OVERRIDES.json > CORPUS_DIR/labels.json
OVERRIDES.json is {case: {statement: true|false}}: a statement listed there is a hit or not as it says,
whatever the judge marked. Each case's labels: {set, hits: [statements that are hits], changed: the
case's changed_files}."""
import glob, json, os, sys, tomllib


def labels(corpus, judged, overrides):
    out = {}
    for f in sorted(glob.glob(os.path.join(corpus, "cases", "*.toml"))):
        c = tomllib.load(open(f, "rb"))
        over = overrides.get(c["id"], {})
        hits = [x["statement"] for x in judged.get(c["id"], []) if over.get(x["statement"], x["hit"])]
        out[c["id"]] = {"set": c["set"], "hits": hits, "changed": c["changed_files"]}
    return out


if __name__ == "__main__":
    corpus, judged, overrides = sys.argv[1:4]
    over = json.load(open(overrides)) if os.path.exists(overrides) else {}
    print(json.dumps(labels(corpus, json.load(open(judged)), over), indent=2, ensure_ascii=False))
