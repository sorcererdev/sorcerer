#!/usr/bin/env bash
# Helpers for the booking scripts.

is_for_table() {
  [ "$1" = "$2" ]
}

function table_label {
  printf "table %s\n" "$1"
}
