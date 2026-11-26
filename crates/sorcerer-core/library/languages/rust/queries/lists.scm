; Sorcerer's own (not vendored): the bracketed lists spellbook's anchors put an item into.
; `parameters in F` is a function's @parameters (not its generics); `call C` puts an argument into
; the call's @arguments, and a struct literal's fields are its arguments (calls.scm names it).
; The items are textobjects.scm's @parameter.inner inside the list.
(function_item parameters: (parameters) @parameters)
(call_expression arguments: (arguments) @arguments)
(struct_expression body: (field_initializer_list) @arguments)
