; Sorcerer's own (not vendored): calls tags.scm does not tag. A composite literal `T{…}` builds
; the type as a constructor call does in other languages.
(composite_literal
  type: (type_identifier) @name) @reference.call
(composite_literal
  type: (qualified_type name: (type_identifier) @name)) @reference.call
