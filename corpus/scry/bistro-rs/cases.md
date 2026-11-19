# bistro-rs cases

32 cases: 10 V-must (one per must rule, a change that breaks it), 10 T (each V-must's twin: the same
scenario following every rule), 6 B (a bug planted in a feature that follows every rule, its test
changed to miss it), 6 N (near misses: a change that looks like it breaks a rule and does not). How
each is made: **nat** is a natural violation by a model run without the conventions, kept only when
it breaks the intended rule; **hand** is derived by a scripted edit from the twin or a feature base;
**file** is a model run with the conventions in its prompt. Every line is `(planned)` until the case
exists.

| id | set | rule | scenario | expected make | status |
|---|---|---|---|---|---|
| vm01 | V-must | ports | guest history: a query listing a guest's past bookings | hand (natural tries likely add a port method) | (planned) |
| vm02 | V-must | web-commands | guest's bookings: a page listing one guest's bookings | hand from t02 (the route calls the repository) | (planned) |
| vm03 | V-must | adapters-main | demo setup: the app starts with a few demo bookings | nat | (planned) |
| vm04 | V-must | rules-in-commands | opening hours: no bookings outside 17:00–22:00 | nat (the check in a route or a model) | (planned) |
| vm05 | V-must | failures-returned | cancel a booking (`CancelBooking`) | nat, else hand from t05 (`panic!`/`String` error) | (planned) |
| vm06 | V-must | statuses-failures | weekly bookings: book the same slot four weeks running | nat (the route matches the failure itself) | (planned) |
| vm07 | V-must | queries-read | next free slot: a query that finds and holds the next free slot | nat | (planned) |
| vm08 | V-must | clock | no bookings in the past | hand from t08 (`SystemTime::now()` in the command) | (planned) |
| vm09 | V-must | repo-one-op | swap two bookings' tables | nat (an adapter `swap` that reads and writes) | (planned) |
| vm10 | V-must | rule-tests | maximum party: no party over 8 | nat (tested only in `routes.rs`) | (planned) |
| t01 | T | ports | guest history | file | (planned) |
| t02 | T | web-commands | guest's bookings | file | (planned) |
| t03 | T | adapters-main | demo setup | file | (planned) |
| t04 | T | rules-in-commands | opening hours | file | (planned) |
| t05 | T | failures-returned | cancel a booking | file | (planned) |
| t06 | T | statuses-failures | weekly bookings | file | (planned) |
| t07 | T | queries-read | next free slot | file | (planned) |
| t08 | T | clock | no bookings in the past | file | (planned) |
| t09 | T | repo-one-op | swap two bookings | file | (planned) |
| t10 | T | rule-tests | maximum party | file | (planned) |
| b01 | B | | change party size: the new party is checked against the old table's seats off by one | hand in feature base f-party | (planned) |
| b02 | B | | one booking a guest a day: the check compares the slot, not the day | hand in feature base f-oneday | (planned) |
| b03 | B | | opening hours: 17:00 itself refused (`>` for `>=`) | hand in t04's feature | (planned) |
| b04 | B | | weekly bookings: the fourth week's slot is not checked | hand in t06's feature | (planned) |
| b05 | B | | swap two bookings: the party-fits check runs against the booking's own table | hand in t09's feature | (planned) |
| b06 | B | | next free slot: the last slot of the day is never offered | hand in t07's feature | (planned) |
| n01 | N | clock | rest of tonight: a model method given the time as an argument | hand | (planned) |
| n02 | N | adapters-main | in-memory app: `main.rs` chooses the in-memory adapter by default | hand | (planned) |
| n03 | N | rules-in-commands | walk-ins: table choice in a private helper fn in the command's module | hand from a feature base | (planned) |
| n04 | N | rules-in-commands | guest name: the route parses and trims the form field | hand | (planned) |
| n05 | N | clock | day summary: the route reads the clock to default the day | file | (planned) |
| n06 | N | repo-one-op | change party size: the command reads then saves (two port calls) | file (f-party) | (planned) |
