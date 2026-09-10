# `!0` and `!:foo` are false: only nil and false are falsy, so zero and a
# symbol are both truthy.
puts !!0
puts !!:foo
puts !!""
puts !nil
puts !false
puts !true
puts !"hello"
__END__
true
true
true
true
true
false
false
