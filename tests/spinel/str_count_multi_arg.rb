# String#count with multiple args computes intersection of charsets
# Each additional arg further restricts which chars to count.

# single arg
puts "hello world".count("lo")
# two args (intersection of "lo" and "o" = just "o")
puts "hello world".count("lo", "o")
# two args with no intersection
puts "hello world".count("l", "o")
# two args with negation
puts "hello world".count("^l", "lo")
