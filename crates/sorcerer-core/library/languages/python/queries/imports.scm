; What a file imports, for conjure's related files: only @import.path is captured, never a
; @name or a definition, so the file's units are unchanged.
(import_statement name: (dotted_name) @import.path)
(import_statement name: (aliased_import name: (dotted_name) @import.path))
(import_from_statement module_name: (dotted_name) @import.path)
