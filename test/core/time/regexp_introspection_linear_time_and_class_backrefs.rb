# linear_time? is false only for a real (non-class) backreference. A
# `\1`/`\k` INSIDE a character class is octal/literal, not a backref, and
# must compile; a forward backref (`/[\]]\1(a)/`) constructs (never matches).

p Regexp.linear_time?(/abc/)
p Regexp.linear_time?(/(a)\1/)
p Regexp.linear_time?(/[\1]/)
p Regexp.linear_time?(/[a-z\k<x>]/)
p Regexp.linear_time?(/[\1](a)\1/)
p Regexp.linear_time?(/[\]]\1(a)/)
p(/[\1]/.match?("\x01"))
p(/[\]]\1(a)/.match("]a").nil?)
__END__
true
false
true
true
false
false
true
true
