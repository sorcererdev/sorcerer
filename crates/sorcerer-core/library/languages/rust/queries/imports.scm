; What a file `use`s, for conjure's related files: only @import.path is captured, never a
; @name or a definition, so the file's units are unchanged.
(use_declaration argument: (scoped_identifier) @import.path)
(use_declaration argument: (scoped_use_list path: (_) @import.path))
(use_declaration argument: (use_as_clause path: (_) @import.path))
(use_declaration argument: (use_wildcard (scoped_identifier) @import.path))
