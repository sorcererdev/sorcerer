; Sorcerer's own (not vendored): a statement that does nothing yet, so a body holding only these
; (and comments) is still empty. Read by core's facts (`@placeholder`). `throw new Error("hole …")`
; is what spellbook writes for a hole; `"not implemented"` is the usual hand-written one.
(throw_statement
  (new_expression
    constructor: (identifier) @_error
    arguments: (arguments . (string (string_fragment) @_message)))
  (#eq? @_error "Error")
  (#match? @_message "^(hole |not implemented)")) @placeholder
