# `String#match(pattern, pos)` starts the search at pos; zeo ignores the
# second argument and matches from 0. `#match?` takes the same argument.
# (Found by the 2026-08-24 probe sweep.)
p "aXbX".match(/X/, 2).begin(0)
p "aXbX".match?(/X/, 2)
p "aXbX".match(/a/, 2)
