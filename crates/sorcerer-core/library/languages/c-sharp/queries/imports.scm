; What a file `using`s, for spellbook's "end of imports" anchor: only @import.path is captured,
; never a @name or a definition, so the file's units are unchanged. A `using` names a namespace,
; not a file, so conjure has no `[imports]` table to resolve it with.
(using_directive (identifier) @import.path)
(using_directive (qualified_name) @import.path)
