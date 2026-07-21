# PostExecutionNode -- `END { ... }`.
#
# CRuby runs END blocks at program exit in REVERSE order of
# registration. Spinel emits each END body as a static C function
# and registers it via atexit() during main() startup.

puts "middle"

END {
  puts "last-1"
}

END {
  puts "last-2"  # registered second; runs first per atexit semantics
}

puts "before-end"
