
## Findings


### Finding 1: bug at a.py:3-3

- statement: a stray blank line
- rule: none cited
- evidence: a.py:3-3
- placed: elsewhere (its lines are in no unit in scope; no unit now): stored with no question
- id 3777d3, elsewhere

### Finding 2: bug at a.py:2-2

- statement: alpha ignores negatives
- rule: cited "no-such-rule": no rule has that id; dropped, the finding stays
- evidence: a.py:2-2
- placed: unit `alpha` (fingerprint 11bc92c)
- same problem as dbfc2d (open, bug at a.py:2; rule: none; alpha adds one)? P(yes) 0.00 → not the same
- same problem as 1464e9 (open, bug at a.py:6; rule: none; beta doubles twice)? P(yes) 0.00 → not the same
- same problem as 3777d3 (elsewhere, bug at a.py:3; rule: none; a stray blank line)? P(yes) 0.00 → not the same
- result: new
- nitpick? P(yes) 0.00 → confidence 1.00, threshold 0.70 → unverified
- id 205301, unverified

### Finding 3: bug at b.py:2-2

- statement: gamma leaks a handle
- rule: none cited
- evidence: b.py:2-2
- placed: unit `gamma` (fingerprint 196c787)
- same problem as e51883 (open, bug at b.py:2; rule: none; gamma leaks)? P(yes) 0.90 → the same: the first at or past the threshold wins
- result: merge into e51883
- id e51883 (merged; seen 2 times), open

### Finding 4: bug at e.py:2-2

- statement: iota may divide by zero
- rule: none cited
- evidence: e.py:2-2
- placed: unit `iota` (fingerprint c6bcf04)
- same problem as 93b8c2 (dismissed-here, bug at e.py:2; rule: none; iota divides by zero)? P(yes) 0.90 → the same: the first at or past the threshold wins
- result: dropped (93b8c2 was dismissed-here here)

### Finding 5: bug at a.py:5-5

- statement: beta has a long name
- rule: none cited
- evidence: a.py:5-5
- placed: unit `beta` (fingerprint 8ec8108)
- same problem as dbfc2d (open, bug at a.py:2; rule: none; alpha adds one)? P(yes) 0.00 → not the same
- same problem as 1464e9 (open, bug at a.py:6; rule: none; beta doubles twice)? P(yes) 0.00 → not the same
- same problem as 3777d3 (elsewhere, bug at a.py:3; rule: none; a stray blank line)? P(yes) 0.00 → not the same
- same problem as 205301 (unverified, bug at a.py:2; rule: none; alpha ignores negatives)? P(yes) 0.00 → not the same
- result: new
- nitpick? P(yes) 0.90 → confidence 0.10, threshold 0.70 → withheld
- id 230ff8, withheld

### Finding 6: bug at b.py:2-2

- statement: gamma never closes its file
- rule: none cited
- evidence: b.py:2-2
- placed: unit `gamma` (fingerprint 196c787)
- same problem as e51883 (open, bug at b.py:2; rule: none; gamma leaks a handle)? P(yes) 0.00 → not the same
- result: new
- nitpick? P(yes) 0.20 → confidence 0.80, threshold 0.70 → unverified
- id 5c4b85, unverified

### Finding 7: bug at c.py:2-2

- statement: eps rounds half to even again
- rule: none cited
- evidence: c.py:2-2
- placed: unit `eps` (fingerprint b4e3383)
- same problem as 2d3fca (withheld, bug at c.py:2; rule: none; eps rounds half to even)? P(yes) 0.90 → the same: the first at or past the threshold wins
- result: merge into 2d3fca
- withheld, and what it was asked on changed: nitpick? P(yes) 0.10 → confidence 0.90, threshold 0.70 → unverified
- id 2d3fca (merged; seen 2 times), unverified

### Finding 8: bug at d.py:2-2

- statement: zeta overflows on big input
- rule: none cited
- evidence: d.py:2-2
- placed: unit `zeta` (fingerprint 14b9556)
- same problem as 05f20c (refuted, bug at d.py:2; rule: none; zeta overflows)? P(yes) 0.90 → the same: the first at or past the threshold wins
- result: merge into 05f20c
- its evidence moved: it is unverified again
- id 05f20c (merged; seen 2 times), unverified

## Remember

- dbfc2d resolved: the reviewer said it no longer holds
- d6b399 gone: its unit or file is no longer there
- 1464e9 resolved: its unit changed and the reviewer did not raise it again

## Verification: a.py


### Round 1

- room: 0 open of 3 shown in the file; 0 of the pass's own opened, of 3
- candidates: 205301 (confidence 1.00)
- left for a later round: none
- 205301: citations hold: `a.py` 2-2 (print 11bc92c)
- 205301 claim check failed: stub: no claim check in these tests → it stays unverified

## Verification: b.py


### Round 1

- room: 1 open of 3 shown in the file; 0 of the pass's own opened, of 3
- candidates: 5c4b85 (confidence 0.80)
- left for a later round: none
- 5c4b85: citations hold: `b.py` 2-2 (print 196c787)
- 5c4b85 claim check failed: stub: no claim check in these tests → it stays unverified

## Verification: c.py


### Round 1

- room: 0 open of 3 shown in the file; 0 of the pass's own opened, of 3
- candidates: 2d3fca (confidence 0.90)
- left for a later round: none
- 2d3fca: citations hold: `c.py` 2-2 (print b4e3383)
- 2d3fca claim check failed: stub: no claim check in these tests → it stays unverified

## Verification: d.py


### Round 1

- room: 0 open of 3 shown in the file; 0 of the pass's own opened, of 3
- candidates: 05f20c (confidence 1.00)
- left for a later round: none
- 05f20c: citations hold: `d.py` 2-2 (print 14b9556)
- 05f20c claim check failed: stub: no claim check in these tests → it stays unverified

## Reviewed

- `a.py` `alpha` reviewed at 11bc92c
- `a.py` `beta` reviewed at 8ec8108
- `b.py` `gamma` reviewed at 196c787
- `c.py` `eps` reviewed at b4e3383
- `d.py` `zeta` reviewed at 14b9556
- `e.py` `iota` reviewed at c6bcf04
