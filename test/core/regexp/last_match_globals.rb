# Last-match globals ($~, $1.., $&, $`, $') and Regexp#~ / Regexp.timeout.

# `=~` populates the last-match globals.
"hello world" =~ /(\w+) (\w+)/
p $~.class.to_s
p $1
p $2
p $&
p $`
p $'
p $~[0]

# A failed match resets them to nil.
"abc" =~ /(\d+)/
p $~
p $1

# `~ /re/` matches the pattern against $_, returning the position (or nil) and
# setting the match globals.
$_ = "the year 2026 arrives"
p(~ /(\d+)/)
p $1
$_ = "no digits here"
p(~ /\d+/)

# Regexp#timeout / Regexp.timeout report the (absent) default.
p(/x/.timeout)
p Regexp.timeout
__END__
"MatchData"
"hello"
"world"
"hello world"
""
""
"hello world"
nil
nil
9
"2026"
nil
nil
nil
