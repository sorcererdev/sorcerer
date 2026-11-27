# Contributing

Thank you for helping. Two rules:

1. **Sign off your commits** (`git commit -s`). The sign-off certifies the
   [Developer Certificate of Origin](https://developercertificate.org/).
2. **The books are part of every change.** `book/user/` (using sorcerer) and
   `book/internals/` (how it works) are kept current in the same branch as the code.
   CI refuses a branch that changes `crates/` without touching `book/`.
   When no chapter needs changing (a pure refactor, a test-only fix), say why in a
   `Book: <reason>` trailer on one of the branch's commits.

Before the first `cargo test`, run `scripts/test-grammars`: it fetches the pinned grammars of
the shipped languages that are not vendored into `target/test-grammars/`, since no test touches
the network. Run it again when a pin in `crates/sorcerer-core/library/languages/` changes.
