# A condition zeo folds at compile time must not eat the condition's own
# EFFECT. `=~` writes `$~`, so `if RUBY_PLATFORM =~ /darwin|linux/` leaves a
# match behind in ruby and the next line may read it.
#
# The build-gate fold (`if RUBY_PLATFORM =~ /mswin/`) is what lets a
# windows-only file compile at all, so it stays -- the match is simply
# re-run for its effect, which is free: the fold succeeded only because both
# operands were static.
#
# A STRING LITERAL subject is not a gate. `if "ab" =~ /a/` is ordinary code,
# and it no longer folds at all.
p("ab" =~ /a/ ? $~[0] : nil)
p($~ && $~[0])
if "ab" =~ /a/
  p $~[0]
  p $1.nil?
end
p RUBY_PLATFORM =~ /darwin|linux/ ? $~[0].is_a?(String) : nil
x = "cd"
p(x =~ /c/ ? $~[0] : nil)
p("zz" =~ /a/ ? "hit" : $~.inspect)
p("ab".match?(/a/) ? 1 : 2)
__END__
"a"
"a"
"a"
true
true
"c"
"nil"
1
