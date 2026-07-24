# Issue #732. `str !~ /pattern/` should return true when no match
# and false when there is a match. spinel used to emit the unresolved-
# call warning and return 0; now `!~` is the negation of `=~`.

puts "hello" !~ /xyz/
puts "hello" !~ /llo/
puts "abc 123" !~ /\d+/
puts "abc" !~ /\d+/
