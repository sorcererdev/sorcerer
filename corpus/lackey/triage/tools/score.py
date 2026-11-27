"""Scores measure.py runs: python3 -I tools/score.py RUN_DIR [KIND_CUT ADDRESSED_CUT] [--sweep]."""
import json, sys
from pathlib import Path

PRICE = 0.042 / 1e6  # dollars per input token; output is free


def acting(row, kind_cut, addressed_cut, use="confidence"):
    kind = row["answers"]["kind"]
    choice = kind.get("choice")
    p = kind.get("confidence") if use == "confidence" else kind.get("probabilities", {}).get(choice, 0)
    addressed = row["answers"]["addressed"].get("noul", 0)
    if choice in (None, "aside", "unsure") or p is None or p < kind_cut or addressed < addressed_cut:
        return None
    return choice


def score(rows, kc, ac, use="confidence"):
    pos = [r for r in rows if r["label"] != "none"]
    neg = [r for r in rows if r["label"] == "none"]
    hit = [r for r in pos if acting(r, kc, ac, use)]
    right = [r for r in pos if (k := acting(r, kc, ac, use)) and (r["label"] == "ask" or k == r["label"])]
    false = [r for r in neg if acting(r, kc, ac, use)]
    return dict(recall=len(hit) / len(pos), kind_right=len(right) / len(pos), hits=len(hit), pos=len(pos),
                false=len(false), neg=len(neg), false_hard=sum(r["hard"] for r in false),
                false_ids=[r["id"] for r in false], missed=[r["id"] for r in pos if r not in hit])


def main():
    run = Path(sys.argv[1])
    args = [a for a in sys.argv[2:] if not a.startswith("--")]
    kc, ac = (float(args[0]), float(args[1])) if args else (0.70, 0.70)
    use = "probability" if "--prob" in sys.argv else "confidence"
    print(f"cuts kind {kc} addressed {ac} ({use})")
    print("| strategy | recall | kind right | false offers (hard) | tokens/triage | ms/triage | $/1k triages |")
    print("|---|---|---|---|---|---|---|")
    for f in sorted(run.glob("*.json")):
        rows = json.loads(f.read_text())
        s = score(rows, kc, ac, use)
        tok = sum(r["input_tokens"] for r in rows) / len(rows)
        ms = 1000 * sum(r["seconds"] for r in rows) / len(rows)
        print(f"| {f.stem} | {s['hits']}/{s['pos']} ({s['recall']:.0%}) | {s['kind_right']:.0%} | "
              f"{s['false']}/{s['neg']} ({s['false_hard']}) | {tok:.0f} | {ms:.0f} | {tok * PRICE * 1000:.4f} |")
        if "--ids" in sys.argv:
            print(f"|  | missed {' '.join(s['missed'])} | | false {' '.join(s['false_ids'])} |")
    if "--sweep" in sys.argv:
        for f in sorted(run.glob("*.json")):
            rows = json.loads(f.read_text())
            best = []
            for kc in [0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8]:
                for ac in [0.5, 0.6, 0.7, 0.8]:
                    s = score(rows, kc, ac, use)
                    best.append((s["hits"] - 2 * s["false"], kc, ac, s["hits"], s["false"]))
            best.sort(reverse=True)
            print(f.stem, "best (hits - 2*false, kind cut, addressed cut, hits, false):", best[:4])


if __name__ == "__main__":
    main()
