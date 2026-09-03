# The block body is a literal replacement (not `m.upcase`) since a
# block parameter's static type is always `Poly` (this codebase's own
# established rule -- params are never inferred from call sites), and
# `String#upcase` itself isn't implemented as a builtin method yet, a
# separate, pre-existing, unrelated gap this test isn't about.

puts "hello".gsub(/l/) { |m| "L" }
puts "hello".sub(/l/) { "L" }
__END__
heLLo
heLlo
