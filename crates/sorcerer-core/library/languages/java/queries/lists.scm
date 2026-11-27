; Sorcerer's own (not vendored): the bracketed lists spellbook's anchors put an item into.
; `parameters in F` is a method's or constructor's @parameters; `call C` puts an argument into
; the call's @arguments (a method call's or a `new X(…)`'s). The items are textobjects.scm's
; @parameter.inner inside the list.
(method_declaration parameters: (formal_parameters) @parameters)
(constructor_declaration parameters: (formal_parameters) @parameters)
(method_invocation arguments: (argument_list) @arguments)
(object_creation_expression arguments: (argument_list) @arguments)
