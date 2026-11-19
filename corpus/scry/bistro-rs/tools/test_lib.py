"""The harness's own checks: python3 -I test_lib.py (offline, no model calls)."""
import json, os, shutil, sys, tempfile

TMP = tempfile.mkdtemp()
os.environ["EXP_CACHE"] = os.path.join(TMP, "cache")
os.environ["EXP_SCRATCH"] = os.path.join(TMP, "work")
os.environ.pop("EXP_LIVE", None)
os.environ["EXP_CEILING"] = "0.05"
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lib  # noqa: E402


def test_the_cache_key_is_stable():
    assert lib.key({"a": 1, "b": [2]}) == lib.key({"b": [2], "a": 1})
    assert lib.key({"a": 1}) != lib.key({"a": 2})


def test_a_replay_without_live_raises_on_a_miss():
    try:
        lib.ask("never asked", "t")
    except RuntimeError as e:
        assert "uncached" in str(e)
    else:
        raise AssertionError("a miss without EXP_LIVE called out")


def test_a_hit_replays_for_free():
    req = {"prompt": "p", "model": "haiku", "tag": "t", "system": ""}
    os.makedirs(os.path.join(lib.CACHE, "claude"), exist_ok=True)
    json.dump({"result": "yes", "cost": 0.01}, open(os.path.join(lib.CACHE, "claude", "ask-" + lib.key(req) + ".json"), "w"))
    assert lib.ask("p", "t")["result"] == "yes"
    assert abs(lib.spend() - 0.01) < 1e-9


def test_the_ceiling_stops_a_live_run():
    lib.LIVE = True
    try:
        lib.ask("would cost", "t", budget=0.1)
    except RuntimeError as e:
        assert "ceiling" in str(e)
    else:
        raise AssertionError("the ceiling did not stop the run")
    finally:
        lib.LIVE = False


def test_json_block_finds_the_last_array():
    assert lib.json_block('text [1] more ```json\n[{"n": 1}]\n```') == [{"n": 1}]


def test_label_lets_an_override_win():
    import label
    corpus = os.path.join(TMP, "corpus")
    os.makedirs(os.path.join(corpus, "cases"))
    for i, st in (("vm01", "V-must"), ("t01", "T")):
        open(os.path.join(corpus, "cases", i + ".toml"), "w").write(
            f'id = "{i}"\nset = "{st}"\nchanged_files = ["src/a.rs"]\n')
    judged = {"vm01": [{"statement": "names it", "hit": True}, {"statement": "near it", "hit": False}],
              "t01": [{"statement": "noise", "hit": False}]}
    overrides = {"vm01": {"near it": True}}
    assert label.labels(corpus, judged, overrides) == {
        "t01": {"set": "T", "hits": [], "changed": ["src/a.rs"]},
        "vm01": {"set": "V-must", "hits": ["names it", "near it"], "changed": ["src/a.rs"]},
    }


if __name__ == "__main__":
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for t in tests:
        t()
        print("ok", t.__name__)
    shutil.rmtree(TMP)
    print(f"{len(tests)} passed")
