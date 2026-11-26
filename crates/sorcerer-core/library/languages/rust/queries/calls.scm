; Sorcerer's own (not vendored): calls tags.scm does not tag. `Type::new(…)` is a call of `new`
; (its whole callee, `Type::new`, names it too), and a struct literal `Type { … }` builds the type
; as a constructor call does in other languages.
(call_expression
  function: (scoped_identifier name: (identifier) @name)) @reference.call
(struct_expression
  name: (type_identifier) @name) @reference.call
