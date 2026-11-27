; What a file imports, for spellbook's "end of imports" anchor: only @import.path is captured,
; never a @name or a definition, so the file's units are unchanged. A Go import names a module
; path (a folder), which conjure has no `[imports]` table to resolve, so it finds no file.
(import_spec path: (interpreted_string_literal) @import.path)
