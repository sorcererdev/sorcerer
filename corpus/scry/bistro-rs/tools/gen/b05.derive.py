# b05 (B), planted in twin t09's feature: only the first booking's move is checked, so the second can land on a table too small for its party or taken at its slot; the tests move only the first booking badly, so they pass
sub("booking/commands/swap_tables.rs", """        self.check(&first_moved, &second.id)?;
        self.check(&second_moved, &first.id)?;""", """        self.check(&first_moved, &second.id)?;""")
