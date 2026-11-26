; Sorcerer's own (not vendored): what stands for a body not written yet, so a body holding only
; these (and comments) is still empty. Read by core's facts (`@placeholder`).
((macro_invocation macro: (identifier) @_macro) @placeholder
  (#any-of? @_macro "todo" "unimplemented"))
(expression_statement
  (macro_invocation macro: (identifier) @_macro)
  (#any-of? @_macro "todo" "unimplemented")) @placeholder
