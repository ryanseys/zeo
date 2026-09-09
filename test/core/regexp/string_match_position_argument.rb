# `String#match(pattern, pos)` starts the search at pos; zeo ignores the
# second argument and matches from 0. `#match?` takes the same argument.
p "aXbX".match(/X/, 2).begin(0)
p "aXbX".match?(/X/, 2)
p "aXbX".match(/a/, 2)
__END__
3
true
nil
