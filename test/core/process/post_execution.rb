# PostExecutionNode -- `END { ... }`.
#
# END blocks run at program exit, in REVERSE order of registration.

puts "middle"

END {
  puts "last-1"
}

END {
  puts "last-2"  # registered second; runs first per atexit semantics
}

puts "before-end"
__END__
middle
before-end
last-2
last-1
