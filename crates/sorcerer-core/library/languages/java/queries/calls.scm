; Sorcerer's own (not vendored): what tags.scm does not tag as a definition or a call. The
; upstream call pattern marks only a call's argument list; here the whole call is the call.
; `new X(…)` is a call of `X`.
(enum_declaration
  name: (identifier) @name) @definition.class
(record_declaration
  name: (identifier) @name) @definition.class
(constructor_declaration
  name: (identifier) @name) @definition.method
(method_invocation
  name: (identifier) @name) @reference.call
(object_creation_expression
  type: (type_identifier) @name) @reference.call
