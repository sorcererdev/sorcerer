# vs1-hand: base main.rs test books 2026-10-09, now in the past; the case refuses past days
sub("main.rs", "(\"day\", \"2026-10-09\")", "(\"day\", \"2099-10-09\")")
