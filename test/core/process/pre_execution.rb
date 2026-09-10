# PreExecutionNode -- `BEGIN { ... }`.
#
# Every BEGIN block runs in source order, BEFORE any other top-level
# statement, wherever in the file it appears.

puts "middle"

BEGIN {
  puts "first"
}

BEGIN {
  puts "first-2"
}

puts "last"
__END__
first
first-2
middle
last
