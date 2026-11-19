# bistro-rs cases

32 cases: 10 V-must (one per must rule, a change that breaks it), 10 T (each V-must's twin: the same
scenario following every rule), 6 B (a bug planted in a feature that follows every rule, its test
changed to miss it), 6 N (near misses: a change that looks like it breaks a rule and does not). How
each is made: **nat** is a natural violation by a model run without the conventions, kept only when
it breaks the intended rule; **hand** is derived by a scripted edit from the twin or a feature base;
**file** is a model run with the conventions in its prompt. The table is written by
`tools/build_cases.py`.

| id | set | rule | scenario | made | how |
|---|---|---|---|---|---|
| b01 | B |  | change party size | hand | hand: tools/gen/b01.derive.py on feature base f-party: the added test's party changed from 4 to 3 |
| b02 | B |  | one booking a guest a day | hand | hand: tools/gen/b02.derive.py on feature base f-oneday: its test books the same slot twice, so it passes |
| b03 | B |  | opening hours | hand | hand: tools/gen/b03.derive.py on twin t04's feature: the test's open slots moved from 17:00 to 17:30 |
| b04 | B |  | weekly bookings | hand | hand: tools/gen/b04.derive.py on twin t06's feature: the taken-week test moved from the last week to the second |
| b05 | B |  | swap two bookings | hand | hand: tools/gen/b05.derive.py on twin t09's feature: its tests move only the first booking badly |
| b06 | B |  | next free slot | hand | hand: tools/gen/b06.derive.py on twin t07's feature: the full-evening test removed |
| n01 | N | clock | rest of tonight | hand | hand: tools/gen/n01.derive.py on the base: a model method told the time by its caller |
| n02 | N | adapters-main | in-memory app | hand | hand: tools/gen/n02.derive.py on the base: main.rs chooses the in-memory adapter by default |
| n03 | N | rules-in-commands | walk-ins | hand | hand: tools/gen/n03.derive.py on feature base f-walkin: the table choice in a private helper fn in the command's module |
| n04 | N | rules-in-commands | guest name | hand | hand: tools/gen/n04.derive.py on the base: the route tidies the guest's name from the form |
| n05 | N | clock | day summary | file | model with the file: sonnet given conventions.md in its prompt (gen.py f-summary): the route reads the clock to default the day, as the base's day page does |
| n06 | N | repo-one-op | change party size | file | model with the file: sonnet given conventions.md in its prompt (gen.py f-party): the command reads the booking, then saves it (two port calls) |
| t01 | T | ports | guest history | file | model with the file: sonnet given conventions.md in its prompt (gen.py t01) and one gen.py --fix run (clippy) |
| t02 | T | web-commands | guest's bookings | file | model with the file: sonnet given conventions.md in its prompt (gen.py t02) |
| t03 | T | adapters-main | demo setup | hand | hand: tools/gen/t03.derive.py on the base: the twin run (gen.py t03) made no change and said a library-side demo constructor would break adapters-main; main.rs gains --demo |
| t04 | T | rules-in-commands | opening hours | file | model with the file: sonnet given conventions.md in its prompt (gen.py t04) |
| t05 | T | failures-returned | cancel a booking | file | model with the file: sonnet given conventions.md in its prompt (gen.py t05) |
| t06 | T | statuses-failures | weekly bookings | file | model with the file: sonnet given conventions.md in its prompt (gen.py t06) |
| t07 | T | queries-read | next free slot | file | model with the file: sonnet given conventions.md in its prompt (gen.py t07) |
| t08 | T | clock | no bookings in the past | file | model with the file: sonnet given conventions.md in its prompt (gen.py t08) |
| t09 | T | repo-one-op | swap two bookings | file | model with the file: sonnet given conventions.md in its prompt (gen.py t09) |
| t10 | T | rule-tests | maximum party | file | model with the file: sonnet given conventions.md in its prompt (gen.py t10) |
| vm01 | V-must | ports | guest history | nat | natural: sonnet without the conventions (gen.py vp1b, prompt in tools/gen.py); vp1a followed the rule (it took the port) |
| vm02 | V-must | web-commands | guest's bookings | hand | hand: tools/gen/vm02.derive.py on twin t02 (its query dropped); the natural try vp2a changed nothing |
| vm03 | V-must | adapters-main | demo setup | nat | natural: sonnet without the conventions (gen.py vp3a, prompt in tools/gen.py) |
| vm04 | V-must | rules-in-commands | opening hours | nat | natural: sonnet without the conventions (gen.py ve2, prompt in tools/gen.py); ve2b put it on Slot and in the route too |
| vm05 | V-must | failures-returned | cancel a booking | hand | hand: tools/gen/vm05.derive.py on twin t05; the natural try fr1 returned the failure |
| vm06 | V-must | statuses-failures | weekly bookings | nat | natural: sonnet without the conventions (gen.py st1, prompt in tools/gen.py); the command it routes is twin t06's (tools/gen/pre-st1.patch) |
| vm07 | V-must | queries-read | next free slot | nat | natural: sonnet without the conventions (gen.py ve9, prompt in tools/gen.py) |
| vm08 | V-must | clock | no bookings in the past | nat | natural: sonnet without the conventions (gen.py vs1, prompt in tools/gen.py); one gen.py --fix run (vs1-fix), then tools/gen/vs1-hand.derive.py moved main.rs's test booking to 2099 |
| vm09 | V-must | repo-one-op | swap two bookings | nat | natural: sonnet without the conventions (gen.py vs4b, prompt in tools/gen.py); vs4c made a SwapTables command with separate port calls |
| vm10 | V-must | rule-tests | maximum party | nat | natural: sonnet without the conventions (gen.py vs5b, prompt in tools/gen.py); vs5 also tested it at the command |
