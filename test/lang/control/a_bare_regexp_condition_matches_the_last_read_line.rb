# A regexp literal written where a condition goes is not the object: ruby
# rewrites it into a match against `$_`, the last line `gets` read, and the
# condition is the match POSITION or nil. `~re` is the same operator spelled
# out, so the two lines below answer alike.
#
# Ruby warns on the form, and the warning is level-sensitive: a PLAIN regexp
# warns in a plain run, an INTERPOLATED one only under `-w`. Both render the
# same sentence, so the node kind is what tells them apart.

$_ = "hello world"

p(/hello/ ? :yes : :no)
p(~/hello/)
p(/world/ ? :yes : :no)
p(/nope/ ? :yes : :no)

puts "modifier" if /lo w/

# The match sets `$~` and its family, as an explicit `=~` does.
p $~[0] if /w(or)ld/
p $1

# An INTERPOLATED regexp is the same match, and warns only under `-w`.
part = "wor"
p(/#{part}ld/ ? :interpolated : :no)

# A Symbol `$_` matches -- `=~` takes one -- and a nil `$_` simply does not.
# Anything else raises, which is `=~`'s own rule rather than a special one:
# `Regexp#~` spells the same idea and answers nil there instead.
$_ = :hello
p(/hell/ ? :symbol : :no)
$_ = nil
p(/hello/ ? :yes : :no)
$_ = 42
begin
  p(/4/ ? :yes : :no)
rescue TypeError => e
  p e.message
end
p(~/4/)
__END__
:yes
0
:yes
:no
modifier
"world"
"or"
:interpolated
:symbol
:no
"no implicit conversion of Integer into String"
nil
#@ stderr
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:12: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:14: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:15: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:17: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:20: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:31: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:33: warning: regex literal in condition
lang/control/a_bare_regexp_condition_matches_the_last_read_line.rb:36: warning: regex literal in condition
