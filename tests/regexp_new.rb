# `Regexp.new` builds a Regexp from a pattern string (or copies another
# Regexp). A second argument sets flags: an Integer OR-ing the `Regexp::`
# constants, or the historical boolean shorthand for case-insensitivity.
re = Regexp.new("ab+c")
p re                                      # /ab+c/
p(re =~ "xabbbc")                         # 1
p re.match?("nope")                       # false

ci = Regexp.new("hello", Regexp::IGNORECASE)
p ci.match?("HELLO")                      # true

# The flag bits combine.
both = Regexp.new("a.b", Regexp::IGNORECASE | Regexp::MULTILINE)
p both.match?("A\nB")                     # true

# `true` is the boolean shorthand for case-insensitive.
p Regexp.new("x", true).match?("X")       # true

# A Regexp source is copied verbatim, flags and all.
p Regexp.new(/foo/i).match?("FOO")        # true
p Regexp.new("ab+c").source               # "ab+c"
