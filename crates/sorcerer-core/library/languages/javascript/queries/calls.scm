; Sorcerer's own (not vendored): calls tags.scm does not tag as calls. `new Type(…)` builds the
; type as a call does, so it counts as a call of `Type` (`new ns.Type(…)`, of `Type`).
(new_expression
  constructor: (identifier) @name) @reference.call
(new_expression
  constructor: (member_expression property: (property_identifier) @name)) @reference.call
