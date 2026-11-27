; What a file sources, for conjure's related files: only @import.path is captured, never a
; @name or a definition, so the file's units are unchanged. `source ./lib.sh` and `. ./lib.sh`.
((command
   name: (command_name (word) @_cmd)
   argument: (word) @import.path)
  (#any-of? @_cmd "source" "."))
((command
   name: (command_name (word) @_cmd)
   argument: (string (string_content) @import.path))
  (#any-of? @_cmd "source" "."))
