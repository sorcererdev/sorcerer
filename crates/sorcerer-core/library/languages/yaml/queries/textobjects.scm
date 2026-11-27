; assignment, statement
(block_mapping_pair
  key: (_) @assignment.lhs
  value: (_) @assignment.rhs) @assignment.outer @statement.outer

(block_mapping_pair
  key: (_) @assignment.inner)

(block_mapping_pair
  value: (_) @assignment.inner)

; number
[
  (integer_scalar)
  (float_scalar)
] @number.inner

(comment) @comment.outer
