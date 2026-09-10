# `str !~ /pattern/` is the negation of `=~`: true when nothing matches,
# false when something does.

puts "hello" !~ /xyz/
puts "hello" !~ /llo/
puts "abc 123" !~ /\d+/
puts "abc" !~ /\d+/
__END__
true
false
false
true
