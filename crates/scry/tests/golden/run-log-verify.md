
## Verification: orders.py


### Round 1

- room: 0 open of 3 shown in the file; 0 of the pass's own opened, of 3
- candidates: 980cf7 (confidence 1.00), 00000a (confidence 0.95), 00000b (confidence 0.92)
- left for a later round: 00000c (confidence 0.90), 00000d (confidence 0.80), 00000e (confidence 0.70), 00000f (confidence 0.60)
- 980cf7: citations hold: `orders.py` 2-2 (print 0e41bd1)
- 00000a: citations fail: cited code not found; `orders.py` line 2: confirmed; `orders.py` line 99: not found → unsure
- 00000b: citations hold: `orders.py` 2-2 (print 0e41bd1)
- 00000b: a convention needs no claim check → open
- 980cf7 claim check: models: haiku; escalated: no; verdict right: Right; cost $0.010 → open
  - cited `orders.py` `cancel` line 2: confirmed

### Round 2

- room: 2 open of 3 shown in the file; 1 of the pass's own opened, of 3
- candidates: 00000c (confidence 0.90)
- left for a later round: 00000d (confidence 0.80), 00000e (confidence 0.70), 00000f (confidence 0.60)
- 00000c: citations hold: `orders.py` 2-2 (print 0e41bd1)
- 00000c claim check failed: stub: no answer → it stays waiting

### Round 3

- room: 2 open of 3 shown in the file; 1 of the pass's own opened, of 3
- candidates: 00000d (confidence 0.80)
- left for a later round: 00000e (confidence 0.70), 00000f (confidence 0.60)
- 00000d: citations hold: `orders.py` 2-2 (print 0e41bd1)
- 00000d claim check: models: haiku; escalated: no; verdict wrong: Wrong; cost $0.010 → refuted
  - cited nothing

### Round 4

- room: 2 open of 3 shown in the file; 1 of the pass's own opened, of 3
- candidates: 00000e (confidence 0.70)
- left for a later round: 00000f (confidence 0.60)
- 00000e: citations hold: `orders.py` 2-2 (print 0e41bd1)
- 00000e: claim check held back: $0.016 more does not fit the caps; it waits
