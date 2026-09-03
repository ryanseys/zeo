# succ carries across a separator only when the alnum kind is preserved
# ("1.9"->"2.0") and otherwise inserts ("a-9"->"a-10"); String#match with a
# block yields the MatchData and answers the block value (nil on a miss).

p "1.9".succ
p "a-9".succ
p "zz9".succ
p "Az9".succ
p "foobar".match(/(o+)/) { |m| m[1].upcase }
p "foobar".match(/xyz/) { |m| m[1] }
p("count=42".match(/(\d+)/) { |m| m[1].to_i * 2 })
__END__
"2.0"
"a-10"
"aaa0"
"Ba0"
"OO"
nil
84
