# Issue #703: !0 / !:foo are false in Ruby (only nil/false are
# falsy). spinel previously used C's `!` directly, treating 0
# as falsy.
puts !!0
puts !!:foo
puts !!""
puts !nil
puts !false
puts !true
puts !"hello"
