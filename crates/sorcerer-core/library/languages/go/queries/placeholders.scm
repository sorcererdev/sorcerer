; Sorcerer's own (not vendored): what stands for a body not written yet, so a body holding only
; these (and comments) is still empty. Read by core's facts (`@placeholder`): a `panic` whose
; message starts `hole ` or `not implemented`.
(expression_statement
  (call_expression
    function: (identifier) @_panic
    arguments: (argument_list (interpreted_string_literal) @_message))
  (#eq? @_panic "panic")
  (#match? @_message "^\"(hole |not implemented)")) @placeholder
