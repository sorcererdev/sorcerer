; Sorcerer's own (not vendored): a statement that does nothing yet, so a body holding only
; these (and comments) is still empty. Read by core's facts (`@placeholder`). Bash's `:` is the
; command that does nothing.
((command name: (command_name (word) @_name)) @placeholder
  (#eq? @_name ":"))
