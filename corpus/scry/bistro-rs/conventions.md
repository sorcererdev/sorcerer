# bistro-rs conventions

Bistro is ports and adapters: `booking/commands` and `booking/queries` hold what the app does,
`booking/models` the data, `booking/ports/<port>/` each port's trait (in `mod.rs`) with its in-memory
and file adapters beside it, `web/<area>/` the routes, and `main.rs` wires it together. A **must**
rule is one we hold every change to; a **taste** rule is how we like it, and nobody needs to stop work
over it.

## Rules

### Commands and queries go through ports (must)

A command or query reaches stored data only through a port's trait. `std::fs` is used in an adapter
(`ports/<port>/file.rs`) and nowhere else.

### The web goes through commands and queries (must)

Code under `web/` calls commands and queries. It never names a port trait, an adapter or `std::fs`.

### Only main.rs chooses adapters (must)

Commands, queries and the web hold a port as `Rc<dyn Port>` and are handed the adapter. Only
`main.rs` (and tests, for their fakes) constructs an in-memory or file adapter.

### Booking rules live in commands (must)

Whether something is allowed, refused or chosen is decided in a command's `handle`. Routes parse the
form and render the result, adapters store and fetch, and models hold data and compute facts about
themselves without enforcing policy.

### Expected failures are returned (must)

Anything a guest can cause (a taken slot, an unknown booking) is a variant of the command's failure
enum, with a `reason`, returned in `Err` from `handle`. We do not `panic!`, `unwrap` or `expect` on
it, and we do not return it as a `String` or a `Box<dyn Error>`, in a command, a model or a route.

### Statuses come from failures.rs (must)

Routes turn a command's failure into an HTTP status by calling its function in
`web/<area>/failures.rs` (`fn <command>_status(&Failure) -> u16`), an exhaustive `match` over the
enum. A route does not pick a failure status itself.

### Queries only read (must)

A query returns data and changes nothing: no `save`, no delete, no clean-up on the side. If reading
needs a write, it is a command.

### Time comes from the clock port (must)

Commands, queries and models never call `SystemTime::now()` or `Instant::now()`. The current time
comes from a `Clock` port (add one the first time you need it), so tests can fix the date.

### Repository methods do one thing (must)

A repository method is one read or one write. When a change needs a read and then a write (move a
booking, cancel one), the command makes both calls; the adapter does not combine them.

### Booking rules are tested at the command (must)

Each booking rule has a test in the command file's `#[cfg(test)] mod tests` that runs it against the
in-memory adapter. Route tests check statuses and pages; they are not where a rule is proven.

### A module says what it is (taste)

Every command, query, port, adapter and web module opens with a one-line `//!` comment saying what
it is for. Models and tests can do without.
