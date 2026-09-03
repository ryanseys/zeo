# Ruby's `&&`/`||` return the actual operand, not a coerced bool --
# `false && x` is `false`, but `5 && x` is `x`, not `true`.

x = 5
y = 3
puts((x > y) && :yes)
puts((x < y) && :yes)
puts((x < y) || :fallback)
puts((x > y) || :fallback)
puts(!(x > y))
puts(!(x < y))
__END__
yes
false
fallback
true
false
true
