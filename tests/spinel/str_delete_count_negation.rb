# String#delete and String#count with ^-negated charset
# ^ at the start inverts the charset meaning.

# delete: normal charset
puts "hello".delete("l").inspect
# delete: negated charset
puts "hello".delete("^l").inspect

# count: normal charset
puts "hello".count("l")
# count: negated charset
puts "hello".count("^l")

# a lone "^" is the literal caret, not negation
puts "a^b^c".delete("^").inspect
puts "a^b^c".count("^")
