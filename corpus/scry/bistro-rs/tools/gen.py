"""Candidates for the cases: sonnet edits a fresh copy of the base, cached (lib.edit_run).

V-must tries (C) run without the conventions, told the scenario and the file(s) to work in; a try is
kept only when its patch breaks the intended rule. Twins (TWINS) run with the conventions in the
prompt. FIX is the follow-up for a candidate that does not compile: the compiler's output pasted in.
Usage: gen.py --list               the ids and prompts; nothing is called
       EXP_LIVE=1 gen.py ID ...    run (or replay) each, print the diff, save gen/ID.patch and gen/ID.json
       EXP_LIVE=1 gen.py --fix ID  one FIX run on gen/ID.patch with check_case.py's output -> gen/ID-fix.patch"""
import json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I leaves the script's directory out
from lib import EXP, HERE, edit_run, fresh_copy, sh, spend  # noqa: E402

TAIL = ("\n\nKeep cargo test and cargo clippy (-D warnings) passing. Do not run anything; just edit. "
        "No new comments. Reply with one line saying what you changed.")

C = {
 # id: (case, scenario, prompt)
 "vp1a": ("vm01", "guest history",
   "Add a query in a new file src/booking/queries/guest_history.rs (register it in src/booking/queries/mod.rs) "
   "that lists every booking a guest has ever made, across all days, oldest first. Work only in those two files."),
 "vp1b": ("vm01", "guest history",
   "Add a GuestHistory query in a new file src/booking/queries/guest_history.rs (register it in "
   "src/booking/queries/mod.rs): given a path to the bookings file bistro.db, list every booking a guest has "
   "ever made, oldest first. Work only in those two files."),
 "vp2a": ("vm02", "guest's bookings",
   "Hosts need to look a guest up: add a route /bookings/guest taking name, listing every booking under that "
   "name on any day. Work only in src/web/bookings/routes.rs, src/web/app.rs and src/main.rs."),
 "vp3a": ("vm03", "demo setup",
   "Scripts and examples should be able to write CreateBooking::demo() and get a working command with a few "
   "demo tables (t1 seats 2, t2 seats 4, t3 seats 6). Do it in src/booking/commands/create_booking.rs only."),
 "ve2": ("vm04", "opening hours",
   "Opening hours: the restaurant takes bookings only from 17:00 to 21:30 and is closed on Mondays. Make the "
   "create-booking route refuse anything else. Work only in src/web/bookings/routes.rs."),
 "ve2b": ("vm04", "opening hours",
   "Opening hours: the restaurant takes bookings only from 17:00 to 21:30 and is closed on Mondays. Put the "
   "check on Slot in src/booking/models/slot.rs and have the create-booking route refuse a slot outside the "
   "hours. Work only in src/booking/models/slot.rs and src/web/bookings/routes.rs."),
 "fr1": ("vm05", "cancel a booking",
   "Bookings can be cancelled by id: add a CancelBooking command in a new file "
   "src/booking/commands/cancel_booking.rs (register it in src/booking/commands/mod.rs), adding what it needs "
   "to the BookingRepository trait and both adapters. Work only in those files."),
 "st1": ("vm06", "weekly bookings",
   "CreateWeeklyBooking (src/booking/commands/create_weekly_booking.rs) is written and tested. Put it on the "
   "web: a route /bookings/create_weekly_booking taking the create-booking form plus weeks, showing the bookings "
   "made. Work only in src/web/bookings/routes.rs, src/web/app.rs and src/main.rs."),
 "ve9": ("vm07", "next free slot",
   "Add a query in a new file src/booking/queries/next_free_slot.rs (register it in src/booking/queries/mod.rs): "
   "given a day, a party and a guest, find the earliest free slot (17:00 to 21:30, every half hour) at a table "
   "that fits, and hold it for the guest so the next caller is not offered the same one. Work only in those files."),
 "vs1": ("vm08", "no bookings in the past",
   "No bookings in the past: refuse a booking for a day before today. Work only in "
   "src/booking/commands/create_booking.rs and src/web/bookings/failures.rs."),
 "vs4b": ("vm09", "swap two bookings",
   "Hosts sometimes swap two bookings' tables. Add swap_tables(first_booking_id, second_booking_id) to the "
   "BookingRepository trait and both adapters. Work only in src/booking/ports/booking_repository/."),
 "vs4c": ("vm09", "swap two bookings",
   "Hosts sometimes swap two bookings' tables. Add a SwapTables command in a new file "
   "src/booking/commands/swap_tables.rs, with the repository support it needs in "
   "src/booking/ports/booking_repository/ (trait and both adapters)."),
 "vs5": ("vm10", "maximum party",
   "Maximum party policy: no booking may be for more than 8 people, whatever the table. Add it to "
   "src/booking/commands/create_booking.rs and src/web/bookings/failures.rs, and make sure the web refuses such "
   "a booking; add a test."),
 "vs5b": ("vm10", "maximum party",
   "Maximum party policy: no booking may be for more than 8 people, whatever the table. Add it to "
   "src/booking/commands/create_booking.rs and src/web/bookings/failures.rs, with a test in "
   "src/web/bookings/routes.rs that the web refuses such a booking."),
}

PRE = {"st1": "gen/pre-st1.patch"}  # id -> patch applied (uncommitted) before the run; the saved diff includes it

TWIN = ("You are working on bistro. Its developers' conventions follow; your change must follow every "
        "must rule and the taste rules too.\n\n<conventions>\n{conv}\n</conventions>\n\nTask: {task} "
        "Change whatever files the conventions call for (port, adapters, command, failures.rs, routes, app.rs, "
        "main.rs, tests), and keep the change as small as the conventions allow.")
TWINS = {
 # id: (case, scenario, task)
 "t01": ("t01", "guest history", "Add a query that lists every booking a guest has ever made, across all days, "
         "oldest first."),
 "t02": ("t02", "guest's bookings", "Hosts need to look a guest up: add a route /bookings/guest taking name, "
         "listing every booking under that name on any day, with a query behind it."),
 "t03": ("t03", "demo setup", "Scripts and examples should be able to get a working app with a few demo tables "
         "(t1 seats 2, t2 seats 4, t3 seats 6) in memory with one call."),
 "t04": ("t04", "opening hours", "Opening hours: the restaurant takes bookings only from 17:00 to 21:30 and is "
         "closed on Mondays. Refuse bookings outside them."),
 "t05": ("t05", "cancel a booking", "Bookings can be cancelled by id; cancelling an unknown booking is refused. "
         "Add the command only (no web route yet)."),
 "t06": ("t06", "weekly bookings", "Regulars book the same table every week: add a command that books a table "
         "at the same time for a number of consecutive weeks, all or nothing, and a route for it."),
 "t07": ("t07", "next free slot", "Given a day, a party and a guest, find the earliest free slot (17:00 to 21:30, "
         "every half hour) at a table that fits, and hold it for the guest so the next caller is not offered the "
         "same one."),
 "t08": ("t08", "no bookings in the past", "No bookings in the past: refuse a booking for a day before today."),
 "t09": ("t09", "swap two bookings", "Hosts sometimes swap two bookings' tables. Add a command that swaps the "
         "tables of two bookings given their ids (no web route yet)."),
 "t10": ("t10", "maximum party", "Maximum party policy: no booking may be for more than 8 people, whatever the "
         "table, and the web refuses such a booking."),
 # feature bases for B and N
 "f-party": ("b01,n06", "change party size", "A booking's party size can be changed by id, within its table's "
         "seats; an unknown booking or a party too large is refused. Add the command only (no web route yet)."),
 "f-oneday": ("b02", "one booking a guest a day", "A guest may hold at most one booking a day. Refuse a second one."),
 "f-walkin": ("n03", "walk-ins", "Walk-ins: add a SeatWalkIn command that seats a party at a given slot at the "
         "smallest free table that fits (refusing when none fits), and a route /bookings/walk_in for it."),
 "f-summary": ("n05", "day summary", "Add a day summary for the manager: for a day, the number of bookings, the "
         "covers (guests expected) and the tables with no booking at all, as a query and a /bookings/summary page "
         "(the day defaults to today)."),
}

FIX = ("The change in this repository does not build or does not pass its checks. The output follows.\n\n"
       "<output>\n{output}\n</output>\n\nFix these errors. Edit only; change nothing the errors do not need.")


def prompt_of(i):
    if i in TWINS:
        conv = open(os.path.join(EXP, "conventions.md")).read()
        return TWIN.format(conv=conv, task=TWINS[i][2]) + TAIL
    return C[i][2] + TAIL


def save(name, rec):
    os.makedirs(os.path.join(HERE, "gen"), exist_ok=True)
    open(os.path.join(HERE, "gen", name + ".patch"), "w").write(rec["diff"])
    json.dump({k: rec.get(k) for k in ("req", "result", "cost", "secs", "turns", "subtype")},
              open(os.path.join(HERE, "gen", name + ".json"), "w"), indent=1)
    print(rec["diff"])
    print(f"--- {name}: ${rec['cost']:.3f} {rec['secs']}s turns={rec.get('turns')} {rec.get('subtype')} "
          f"| total ${spend():.2f}")
    print(rec["result"][:400])


def run(i):
    d = fresh_copy("gen-" + i)
    if i in PRE:
        sh(["git", "apply", os.path.join(HERE, PRE[i])], cwd=d)
    save(i, edit_run(prompt_of(i), d, tag=i))


def fix(i):
    from check_case import check
    patch = os.path.join(HERE, "gen", i + ".patch")
    steps = check(patch)
    if all(ok for _, ok, _ in steps) and len(steps) == 3:
        print(i, "passes its checks; no fix needed")
        return
    d = fresh_copy("check")
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(os.path.dirname(d), "target"), CARGO_TERM_COLOR="never")
    failing = [n for n, ok, _ in steps if not ok][0]
    sh(["git", "apply", "--whitespace=nowarn", patch], cwd=d)
    args = {"check": ["check", "--all-targets"], "test": ["test"],
            "clippy": ["clippy", "--all-targets", "--", "-D", "warnings"]}[failing]
    r = sh(["cargo", args[0], "--offline", "--locked", *args[1:]], cwd=d, cap=600, check=False, env=env)
    output = (r.stdout + r.stderr)[-6000:]
    w = fresh_copy("gen-" + i + "-fix")
    sh(["git", "apply", "--whitespace=nowarn", patch], cwd=w)
    save(i + "-fix", edit_run(FIX.format(output=output), w, tag=i + "-fix"))


if __name__ == "__main__":
    args = sys.argv[1:]
    if args == ["--list"]:
        for i in list(C) + list(TWINS):
            case, scen = (C.get(i) or TWINS[i])[:2]
            print(f"== {i} ({case}: {scen}){' after ' + PRE[i] if i in PRE else ''}")
            print(prompt_of(i) if i in C else TWINS[i][2])
        print(f"{len(C)} V-must tries, {len(TWINS)} twins and feature bases")
    elif args[:1] == ["--fix"]:
        for i in args[1:]:
            fix(i)
    else:
        for i in args:
            run(i)
