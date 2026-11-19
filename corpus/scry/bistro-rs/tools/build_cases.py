"""Writes the cases from the chosen patches in gen/: cases/<id>.patch, cases/<id>.toml (planted lines
found in the patched tree, changed files from the patch) and the table in ../cases.md.
Usage: build_cases.py   (offline; rerun after any change to CASES or a patch in gen/)"""
import os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I leaves the script's directory out
from lib import EXP, HERE, fresh_copy, sh  # noqa: E402

CLASS = {"ports": "pattern", "web-commands": "pattern", "adapters-main": "pattern",
         "rules-in-commands": "evidenced", "failures-returned": "evidenced", "statuses-failures": "evidenced",
         "queries-read": "evidenced", "clock": "stated-only", "repo-one-op": "stated-only",
         "rule-tests": "stated-only"}
NAT = "natural: sonnet without the conventions (gen.py {}, prompt in tools/gen.py)"
FILE = "model with the file: sonnet given conventions.md in its prompt (gen.py {})"
HAND = "hand: tools/gen/{}.derive.py on {}"

# id: (set, scenario, gen patch, rule, planted [(file, [substring, ...])], expect, made, twin)
CASES = {
 "vm01": ("V-must", "guest history", "vp1b", "ports",
   [("booking/queries/guest_history.rs", ["use std::fs;", "fs::read_to_string"])],
   "GuestHistory reads the bookings file itself with std::fs instead of going through the BookingRepository port; std::fs belongs in an adapter.",
   NAT.format("vp1b") + "; vp1a followed the rule (it took the port)", "t01"),
 "vm02": ("V-must", "guest's bookings", "vm02", "web-commands",
   [("web/bookings/routes.rs", ["app.bookings.bookings_for"]), ("web/app.rs", ["pub bookings: Rc<dyn BookingRepository>"])],
   "The guest page route calls the BookingRepository port (app.bookings.bookings_for) itself instead of a query; the web must go through commands and queries.",
   HAND.format("vm02", "twin t02 (its query dropped)") + "; the natural try vp2a changed nothing", "t02"),
 "vm03": ("V-must", "demo setup", "vp3a", "adapters-main",
   [("booking/commands/create_booking.rs", ["InMemoryBookingRepository::new(tables)", "SequentialIds::default()),"])],
   "CreateBooking::demo() builds the in-memory adapters itself; only main.rs (or a test) chooses and constructs adapters.",
   NAT.format("vp3a"), "t03"),
 "vm04": ("V-must", "opening hours", "ve2", "rules-in-commands",
   [("web/bookings/routes.rs", ["fn is_open(", "if !is_open("])],
   "The opening-hours rule (17:00 to 21:30, closed Mondays) is decided in the create-booking route instead of in CreateBooking::handle.",
   NAT.format("ve2") + "; ve2b put it on Slot and in the route too", "t04"),
 "vm05": ("V-must", "cancel a booking", "vm05", "failures-returned",
   [("booking/commands/cancel_booking.rs", ["panic!(\"there is no booking"])],
   "CancelBooking::handle panics on an unknown booking id, which a guest can cause, instead of returning a failure variant in Err.",
   HAND.format("vm05", "twin t05") + "; the natural try fr1 returned the failure", "t05"),
 "vm06": ("V-must", "weekly bookings", "st1", "statuses-failures",
   [("web/bookings/routes.rs", ["fn create_weekly_booking_status("])],
   "The weekly-booking route maps CreateWeeklyBookingFailure to HTTP statuses in routes.rs itself instead of a create_weekly_booking_status function in web/bookings/failures.rs.",
   NAT.format("st1") + "; the command it routes is twin t06's (tools/gen/pre-st1.patch)", "t06"),
 "vm07": ("V-must", "next free slot", "ve9", "queries-read",
   [("booking/queries/next_free_slot.rs", ["self.bookings.save("])],
   "NextFreeSlot is a query but saves a booking to hold the slot; a query changes nothing, so holding a slot is a command.",
   NAT.format("ve9"), "t07"),
 "vm08": ("V-must", "no bookings in the past", "vs1-hand", "clock",
   [("booking/commands/create_booking.rs", ["use std::time::{SystemTime, UNIX_EPOCH};", "SystemTime::now()"])],
   "CreateBooking reads SystemTime::now() to tell whether the day is past instead of getting the date from a Clock port.",
   NAT.format("vs1") + "; one gen.py --fix run (vs1-fix), then tools/gen/vs1-hand.derive.py moved main.rs's test booking to 2099", "t08"),
 "vm09": ("V-must", "swap two bookings", "vs4b", "repo-one-op",
   [("booking/ports/booking_repository/mod.rs", ["fn swap_tables("]),
    ("booking/ports/booking_repository/in_memory.rs", ["fn swap_tables("]),
    ("booking/ports/booking_repository/file.rs", ["fn swap_tables("])],
   "swap_tables on the BookingRepository port reads both bookings and writes them back in one repository method; the reads and writes belong in a command.",
   NAT.format("vs4b") + "; vs4c made a SwapTables command with separate port calls", "t09"),
 "vm10": ("V-must", "maximum party", "vs5b", "rule-tests",
   [("web/bookings/routes.rs", ["fn a_party_over_the_limit_is_refused_even_at_a_big_table("])],
   "The maximum-party rule is tested only through the route in routes.rs; it has no test in create_booking.rs's mod tests against the in-memory adapter.",
   NAT.format("vs5b") + "; vs5 also tested it at the command", "t10"),
 "b01": ("B", "change party size", "b01", "",
   [("booking/commands/change_party_size.rs", ["if input.party >= table.seats {"])],
   "ChangePartySize refuses a party equal to the table's seats (>= where CreateBooking uses >), so a party that exactly fills its table cannot be set.",
   HAND.format("b01", "feature base f-party") + ": the added test's party changed from 4 to 3", ""),
 "b02": ("B", "one booking a guest a day", "b02", "",
   [("booking/commands/create_booking.rs", ["existing.guest == input.guest && existing.slot == input.slot"])],
   "The once-a-day check refuses only a second booking at the same slot; the same guest can still book twice on one day at different times.",
   HAND.format("b02", "feature base f-oneday") + ": its test books the same slot twice, so it passes", ""),
 "b03": ("B", "opening hours", "b03", "",
   [("booking/commands/create_booking.rs", ["input.slot.start <= Time::new(17, 0)"])],
   "The opening-hours check refuses 17:00 itself (<= 17:00), though bookings are taken from 17:00.",
   HAND.format("b03", "twin t04's feature") + ": the test's open slots moved from 17:00 to 17:30", ""),
 "b04": ("B", "weekly bookings", "b04", "",
   [("booking/commands/create_weekly_booking.rs", ["for slot in &slots[..slots.len() - 1] {"])],
   "The taken-slot check skips the last week (slots[..len - 1]), so a weekly booking can double-book its table in its final week.",
   HAND.format("b04", "twin t06's feature") + ": the taken-week test moved from the last week to the second", ""),
 "b05": ("B", "swap two bookings", "b05", "",
   [("booking/commands/swap_tables.rs", ["self.check(&first_moved, &second.id)?;"])],
   "Only the first booking's move is checked: the second booking can be moved to a table too small for its party or already taken at its slot.",
   HAND.format("b05", "twin t09's feature") + ": its tests move only the first booking badly", ""),
 "b06": ("B", "next free slot", "b06", "",
   [("booking/commands/hold_earliest_slot.rs", ["for half_hours in 0..9 {"])],
   "The search covers only nine half hours (0..9), so the last slot of the evening, 21:30, is never offered.",
   HAND.format("b06", "twin t07's feature") + ": the full-evening test removed", ""),
 "n01": ("N", "rest of tonight", "n01", "clock", [], "", HAND.format("n01", "the base") + ": a model method told the time by its caller", ""),
 "n02": ("N", "in-memory app", "n02", "adapters-main", [], "", HAND.format("n02", "the base") + ": main.rs chooses the in-memory adapter by default", ""),
 "n03": ("N", "walk-ins", "n03", "rules-in-commands", [], "", HAND.format("n03", "feature base f-walkin") + ": the table choice in a private helper fn in the command's module", ""),
 "n04": ("N", "guest name", "n04", "rules-in-commands", [], "", HAND.format("n04", "the base") + ": the route tidies the guest's name from the form", ""),
 "n05": ("N", "day summary", "f-summary", "clock", [], "", FILE.format("f-summary") + ": the route reads the clock to default the day, as the base's day page does", ""),
 "n06": ("N", "change party size", "f-party", "repo-one-op", [], "", FILE.format("f-party") + ": the command reads the booking, then saves it (two port calls)", ""),
}
TWINS = {"t01": ("guest history", "t01-fix", "ports", "vm01", " and one gen.py --fix run (clippy)"),
         "t02": ("guest's bookings", "t02", "web-commands", "vm02", ""), "t03": ("demo setup", "t03-hand", "adapters-main", "vm03", ""),
         "t04": ("opening hours", "t04", "rules-in-commands", "vm04", ""), "t05": ("cancel a booking", "t05", "failures-returned", "vm05", ""),
         "t06": ("weekly bookings", "t06", "statuses-failures", "vm06", ""), "t07": ("next free slot", "t07", "queries-read", "vm07", ""),
         "t08": ("no bookings in the past", "t08", "clock", "vm08", ""), "t09": ("swap two bookings", "t09", "repo-one-op", "vm09", ""),
         "t10": ("maximum party", "t10", "rule-tests", "vm10", "")}
for t, (scen, patch, rule, twin, extra) in TWINS.items():
    CASES[t] = ("T", scen, patch, rule, [], "", FILE.format(t) + extra, twin)
CASES["t03"] = CASES["t03"][:6] + (HAND.format("t03", "the base") + ": the twin run (gen.py t03) made no change and said a library-side demo constructor would break adapters-main; main.rs gains --demo", "vm03")


def lines_of(text, subs):
    out, in_tests = [], False
    for n, line in enumerate(text.split("\n"), 1):
        if any(s in line for s in subs):
            out.append(n)
    return out


def ranges(ns):
    out, start = [], None
    for i, n in enumerate(ns):
        if start is None:
            start = n
        if i + 1 == len(ns) or ns[i + 1] != n + 1:
            out.append(str(start) if start == n else f"{start}-{n}")
            start = None
    return ",".join(out)


def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    os.makedirs(os.path.join(EXP, "cases"), exist_ok=True)
    rows = []
    for cid, (st, scen, patch, rule, planted, expect, made, twin) in sorted(CASES.items()):
        text = open(os.path.join(HERE, "gen", patch + ".patch")).read()
        open(os.path.join(EXP, "cases", cid + ".patch"), "w").write(text)
        changed = sorted(set(re.findall(r"^\+\+\+ b/(\S+)", text, flags=re.M)))
        d = fresh_copy("build-" + cid)
        sh(["git", "apply", "--whitespace=nowarn", os.path.join(EXP, "cases", cid + ".patch")], cwd=d)
        spots = []
        for f, subs in planted:
            ns = lines_of(open(os.path.join(d, "src", f)).read(), subs)
            if not ns:
                raise SystemExit(f"{cid}: nothing planted found in {f}")
            spots.append(f"src/{f}:{ranges(ns)}")
        toml = [f"id = {q(cid)}", f"set = {q(st)}", f"scenario = {q(scen)}", f"patch = {q('cases/' + cid + '.patch')}",
                f"rule = {q(rule)}", f"rule_class = {q(CLASS.get(rule, ''))}", f"rule_broken = {'true' if st == 'V-must' else 'false'}",
                "planted_lines = [" + ", ".join(q(s) for s in spots) + "]", f"expect = {q(expect)}", f"made = {q(made)}",
                f"twin = {q(twin)}", "changed_files = [" + ", ".join(q(c) for c in changed) + "]"]
        open(os.path.join(EXP, "cases", cid + ".toml"), "w").write("\n".join(toml) + "\n")
        how = "nat" if made.startswith("natural") else "file" if made.startswith("model") else "hand"
        rows.append(f"| {cid} | {st} | {rule} | {scen} | {how} | {made} |")
    md = open(os.path.join(EXP, "cases.md")).read()
    head = md.split("\n| id |")[0].replace("Every line is `(planned)` until the case\nexists.", "The table is written by\n`tools/build_cases.py`.")
    open(os.path.join(EXP, "cases.md"), "w").write(head + "\n| id | set | rule | scenario | made | how |\n|---|---|---|---|---|---|\n" + "\n".join(rows) + "\n")
    print(len(rows), "cases written")


if __name__ == "__main__":
    main()
