
## Summary

- dddd01 convention at orders.py:6: open; not printed: its unit is not in this run's scope
- ab2da0 convention at orders.py:2: open; printed
- 0ed883 design at orders.py:2: open; not printed: its issue type design is not in [scry] issue_types
- 5e44e4 bug at orders.py:2: refuted (wrong: it checks the state first); not printed: the claim check said it is wrong
- 75adf8 bug at orders.py:2: unsure (cannot_tell: the refund is not in view); not printed: the check could not confirm it
- 590eb9 bug at orders.py:2: waiting; not printed: not checked yet (a cap or a failed check)
- 3f3e01 bug at orders.py:2: unsure (cited code not found: ); not printed: the check could not confirm it
- eaab9e bug at orders.py:2: withheld; not printed: judged a nitpick (confidence 0.10 under the threshold 0.70)
- e05bac bug at orders.py:3: elsewhere; not printed: its lines are in no unit in scope
- spent: the pass charged $0.040; the ledger's new scry rows: review 2 for $0.020
- failure: review failed for x_big.py: no JSON in the review reply
- capped: 1 unit
- a claim check was held back by the caps

**Result:** 1 finding shown, 1 unit looked at, $0.04, exit 1
