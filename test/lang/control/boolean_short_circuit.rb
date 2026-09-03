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
