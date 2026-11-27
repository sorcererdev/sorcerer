; Sorcerer's own (not vendored): the bracketed lists spellbook's anchors put an item into.
; `parameters in F` is a function's or method's @parameters (not its receiver or results);
; `call C` puts an argument into the call's @arguments, and a composite literal's elements are
; its arguments (calls.scm names it). The items are textobjects.scm's @parameter.inner.
(function_declaration parameters: (parameter_list) @parameters)
(method_declaration parameters: (parameter_list) @parameters)
(call_expression arguments: (argument_list) @arguments)
(composite_literal body: (literal_value) @arguments)
