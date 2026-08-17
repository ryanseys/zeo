# Five String surfaces that answered where CRuby refuses, or refused where
# CRuby answers.

# an empty pad is rejected whatever the width -- the no-op width case used to
# return the receiver before looking at the pad
r = ("hi".ljust(5, "") rescue $!.class); p r
r = ("hi".rjust(5, "") rescue $!.class); p r
r = ("hi".center(5, "") rescue $!.class); p r
r = ("hi".ljust(1, "") rescue $!.class); p r
p "hi".ljust(5, ".")
p "hi".rjust(5, ".")
p "hi".center(6, ".")

# a radix outside 2..36 is an error, not a silent fallback to 10
r = ("12".to_i(1) rescue $!.class); p r
r = ("12".to_i(37) rescue $!.class); p r
p "12".to_i(0)
p "ff".to_i(16)
p "12".to_i

# both prefix predicates take any number of candidates, so none is false
p "hello".start_with?
p "hello".end_with?
p "hello".start_with?("he")
p "hello".end_with?("lo", "xx")

# #b answers a fresh mutable copy, not the frozen receiver
p "abc".b.frozen?
s = "abc".b
s << "d"
p s

# `str =~ str` is a TypeError, not a missing method
r = ("hello" =~ "l" rescue $!.class); p r
p("hello" =~ /l/)
