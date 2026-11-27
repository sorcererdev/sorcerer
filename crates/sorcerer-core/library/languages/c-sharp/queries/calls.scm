; Sorcerer's own (not vendored): what tags.scm does not tag as a definition or a call. The
; upstream file calls a method call `@reference.send`; here it is a `@reference.call`, and so is
; `new X(…)`. Structs, enums, records and constructors are definitions.
(struct_declaration
  name: (identifier) @name) @definition.class
(enum_declaration
  name: (identifier) @name) @definition.class
(record_declaration
  name: (identifier) @name) @definition.class
(constructor_declaration
  name: (identifier) @name) @definition.method
(invocation_expression
  function: (member_access_expression name: (identifier) @name)) @reference.call
(invocation_expression
  function: (identifier) @name) @reference.call
(object_creation_expression
  type: (identifier) @name) @reference.call
