; Sorcerer's own (not vendored): a comment, and a statement, for `start of` / `end of` a statement.
(comment) @comment.outer
(statement) @statement.outer

; what a definition holds, so a table, view, index or function is not mistaken for an empty stub
(create_table (column_definitions) @class.inner)
(create_view (create_query) @class.inner)
(create_index (index_fields) @class.inner)
(create_function (function_body) @function.inner)
