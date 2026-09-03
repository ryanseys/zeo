# Comparable's operators and Kernel's universals reach builtin values
# dynamically: `"abc" < "abd"` resolves String(no `<`) -> Comparable(`<`
# drives `<=>`) -> `String#<=>`; itself/tap/then/frozen?/eql?/equal? are
# Kernel/BasicObject rows found through every value's chain.

x = "abc"
puts x < "abd"
puts x.between?("aaa", "b")
puts "m".clamp("a", "f")
puts 5.itself
r = 5.tap { |v| puts "saw #{v}" }
puts r
puts 5.then { |v| v + 1 }
puts 5.equal?(5)
puts 5.send("itself")
puts 5.frozen?
puts "x".frozen?
puts 5.eql?(5.0)
puts 5.eql?(5)
__END__
true
true
f
5
saw 5
5
6
true
5
true
false
false
true
