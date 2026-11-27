; Sorcerer's own (not vendored): what a SQL file defines, each named by its object_reference.
(create_table
    (object_reference name: (identifier) @name)) @definition.class

(create_view
    (object_reference name: (identifier) @name)) @definition.class

(create_function
    (object_reference name: (identifier) @name)) @definition.function

(create_index
    column: (identifier) @name) @definition.class
