# Languages

Sorcerer ships sixteen languages. Each is two kinds of file under
`crates/sorcerer-core/library/languages/<name>/` (a `language.toml` and its tree-sitter
queries); the tools read them and know no language themselves. A language you need that is not
here is [added the same way](adding-a-language.md), with no code.

Each tool does what a language's files let it do. A *code* language has units (functions,
methods, classes and the like), so it is reviewed, conjured in and has stubs; a config or markup
format has no units, so it gets the rest: comments, rules, generators and owned regions.

| language | files | scry | conjure | spellbook | lackey |
|---|---|---|---|---|---|
| Python | `.py` | reviewed | test file `x__test.py`, imports | rules, holes, regions `#`, anchors, fixes | comments, stubs |
| Rust | `.rs` | reviewed | tests in the file itself, `use` imports | rules, holes, regions `//`, anchors, fixes | comments, stubs |
| TypeScript | `.ts`, `.mts`, `.cts` | reviewed | test file `x.test.ts`, relative imports | rules, holes, regions `//`, anchors, fixes | comments, stubs |
| TSX | `.tsx` | reviewed | test file `x.test.tsx`, relative imports (`.tsx` and `.ts`) | rules, holes, regions `//`, anchors, fixes | comments, stubs |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | reviewed | test file `x.test.js`, relative imports and `require` | rules, holes (no `type`), regions `//`, anchors, fixes | comments, stubs |
| Go | `.go` | reviewed | test file `x_test.go`; imports not followed | rules, holes, regions `//`, anchors, fixes | comments, stubs |
| Java | `.java` | reviewed | test file `src/test/…/XTest.java` beside `src/main/…`, imports under `src/main/java` | rules, holes, regions `//`, anchors, fixes | comments, stubs |
| C# | `.cs` | reviewed | no test file, imports not followed | rules, holes, regions `//`, anchors, fixes | comments, stubs |
| Bash | `.sh`, `.bash` | reviewed (functions) | no test file; `source ./x.sh` followed | rules, holes (`expr`, `block`, `stmt`), regions `#`, `function NAME` anchors, fixes | comments, stubs (`:`) |
| SQL | `.sql` | reviewed (`CREATE TABLE`, `VIEW`, `INDEX`, `FUNCTION`) | the file alone | rules, holes (`expr`, `stmt`), regions `--` | comments |
| HTML | `.html`, `.htm` | not reviewed | does not run | rules, holes (`block`), regions `<!-- -->` | comments |
| CSS | `.css` | not reviewed | does not run | rules, holes (`block`, `expr`), regions `/* */` | comments |
| Markdown | `.md`, `.markdown` | not reviewed | does not run | rules, holes (`block`), regions `<!-- -->` | prose, by paragraph |
| YAML | `.yml`, `.yaml` | not reviewed | does not run | rules, holes (`expr`), regions `#` | comments |
| TOML | `.toml` | not reviewed | does not run | rules, holes (`expr`), regions `#` | comments |
| JSON | `.json` | not reviewed | does not run | rules, holes (`expr`); no regions or allow comments | — |

What the columns mean:

- **scry**: *reviewed* means scry reads the changed units of the file. A file of a language with
  no units is left out of a review (`skipped (no units)` in the run log): editing a README or a
  `package.json` costs nothing, and its grammar is not even downloaded.
- **conjure**: the test file and the imported files it reads beside the function you ask in
  ([What conjure reads](conjure.md#what-conjure-reads)). With neither, conjure still works, from
  the file alone. It does not run in a file with no units.
- **spellbook**: every language runs `spellbook check` rules and writes generators' files. The
  hole kinds are the `[holes]` a generator may use there (*holes* alone means all five: `ident`,
  `type`, `expr`, `block`, `stmt`). *Regions* shows the comment the markers are written in.
  *Anchors* are the places an `insert` edit or a fix can name (`end of parameters in …`,
  `start of function …`, `end of imports`); see
  [What anchors and fixes read](adding-a-language.md#what-anchors-and-fixes-read).
- **lackey**: *comments* means a comment you write is triaged (an instruction offered, a
  question answered on save); *stubs* means a function whose body does nothing yet (`pass`,
  `todo!()`, `throw new Error("not implemented")`, `panic(…)`, `:`) is offered; *prose* means a
  paragraph you write is triaged as text.

## Grammars

The first time a tool needs a language's grammar, it downloads the prebuilt library for your
platform from the release of your tool's version, checks it, and keeps it in your cache
([Adding a language](adding-a-language.md) says where and what happens offline). Only the
languages your files use are downloaded, once per version: the largest, SQL, is about 13 MB, C#
about 7 MB, and most are under 1.5 MB. `scry doctor` lists the languages of your project and how
each grammar got there.

## Each language's limits

- **TypeScript and TSX** are two languages from one grammar repository, because neither
  grammar parses the other's syntax: a `.ts` file is never read as TSX. JavaScript's grammar
  parses JSX, so `.jsx` is JavaScript. An ESM import written with its `.js` suffix
  (`./models.js` for `models.ts`) is not followed.
- **Go** imports name a module path (`github.com/acme/app/booking`), not a file, so conjure does
  not follow them; the test file still comes along. A method's unit is named without its type
  (`IsFor`).
- **Java** tests are found in the Maven layout: `src/main/java/a/B.java` has its tests in
  `src/test/java/a/BTest.java`. A file outside `src/main/` has no known test file.
- **C#** has neither a test-file convention nor followed imports (`using` names a namespace, not
  a file). A file-scoped `namespace` does not prefix unit names. In config, the language is
  written `c-sharp` (`language = "c-sharp"` in a generator).
- **Bash** units are functions (`name() { … }` and `function name { … }`): a script's top-level
  lines are no unit.
- **SQL** is DerekStride's broad dialect, not one vendor's: vendor syntax (such as Postgres
  `DO $$ … $$`) may parse with errors. Units are the `CREATE` statements; there are no stubs and
  no list anchors (`parameters in`, `call`).
- **Markdown** is the block grammar only: rules see paragraphs, headings, lists and code blocks,
  never inline links or emphasis. Lackey cuts a change in prose by paragraph, as it did before
  Markdown had a grammar. An HTML comment (`<!-- … -->`) is a comment; another HTML block is not.
- **JSON** has no comments, so it has no allow comments, no owned regions, and nothing for
  lackey to read.
- **HTML, CSS, YAML, TOML** have no units: they are reviewed by your rules, not by scry.

## When an existing project upgrades

Files that had no language before (`.html`, `.css`, `.md`, `.toml`, `.yml`, `.json`, and the new
code languages) now have one. `spellbook check` and the doctors load their grammars, which means
a download the first time (offline, a `couldn't download` row until you are back online), and
lackey cuts comments and hunks in them where it cut paragraphs before (Markdown keeps its
paragraphs). Scry and conjure do not load the grammar of a language with no units.
