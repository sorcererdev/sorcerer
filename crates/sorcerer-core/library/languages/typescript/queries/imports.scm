; What a file imports, for conjure's related files: only @import.path is captured, never a
; @name or a definition, so the file's units are unchanged. `import … from "./x"`,
; `export … from "./x"` and `require("./x")` each name a path (a package name such as "react"
; names no file in the project and so resolves to nothing).
(import_statement source: (string (string_fragment) @import.path))
(export_statement source: (string (string_fragment) @import.path))
(call_expression
  function: (identifier) @_callee
  arguments: (arguments . (string (string_fragment) @import.path) .)
  (#eq? @_callee "require"))
