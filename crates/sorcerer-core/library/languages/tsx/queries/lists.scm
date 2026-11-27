; Sorcerer's own (not vendored): the bracketed lists spellbook's anchors put an item into.
; `parameters in F` is a definition's @parameters; `call C` puts an argument into the call's
; @arguments (a `new` expression's too). The items are textobjects.scm's @parameter.inner inside
; the list.
(function_declaration parameters: (formal_parameters) @parameters)
(generator_function_declaration parameters: (formal_parameters) @parameters)
(function_expression parameters: (formal_parameters) @parameters)
(generator_function parameters: (formal_parameters) @parameters)
(method_definition parameters: (formal_parameters) @parameters)
(arrow_function parameters: (formal_parameters) @parameters)
(call_expression arguments: (arguments) @arguments)
(new_expression arguments: (arguments) @arguments)
(function_signature parameters: (formal_parameters) @parameters)
(method_signature parameters: (formal_parameters) @parameters)
(abstract_method_signature parameters: (formal_parameters) @parameters)
