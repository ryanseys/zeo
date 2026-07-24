# PreExecutionNode -- `BEGIN { ... }`.
#
# CRuby runs all BEGIN blocks in source order, BEFORE any other
# top-level statements, regardless of where they appear in the file.
# Spinel hoists each BEGIN body to the top of main() in source-encounter
# order during a pre-pass.

puts "middle"

BEGIN {
  puts "first"
}

BEGIN {
  puts "first-2"
}

puts "last"
