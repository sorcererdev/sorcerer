# sorcerer-core

The shared base of scry, conjure, spellbook and lackey: projects, config, languages, code units, the findings store, the cost ledger and fences (paths a model never sees). A library with no commands.

## Who uses it

Every Sorcerer program. You depend on it only to build another tool on the same base.

## Main parts

- `domain/`: data and rules, no I/O (`Project`, `Config`, `unit`, `compare`, `fences`).
- `ports/`: the seams products talk through (`CodebaseView`, `Ledger`, `Env`).
- `adapters/`: the disk, git, grammars and network behind those seams.

## Example

Print a file's units and how they changed against HEAD, from inside a project:

```sh
cargo run -p sorcerer-core --example units -- src/orders.py
```

```rust
let project = sorcerer_core::domain::project::Project::open(&std::env::current_dir()?)?;
```

Book: [What Sorcerer is](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/what-sorcerer-is.md) (using it), [The core](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/core.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
