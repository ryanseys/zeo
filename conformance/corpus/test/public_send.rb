# Issue #735. `recv.public_send(:meth, args)` should be identical to
# `recv.send(:meth, args)` in spinel (visibility is not modeled in
# static dispatch). The parser textually rewrites `.send(:foo)` and
# `.__send__(:foo)` to `.foo`; now `.public_send(:foo)` (and the
# string-form variant) take the same shortcut.

puts "hello".public_send(:upcase)
puts "world".public_send("upcase")
puts 42.public_send(:to_s)

# __send__ also keeps working (existing path).
puts "x".__send__(:upcase)
