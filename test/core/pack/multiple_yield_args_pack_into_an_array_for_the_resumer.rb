# Asserted via `puts` (an Array prints one element per line, matching
# real ruby) rather than `v.length`/`v[0]`: `resume`'s result is
# statically Poly, and collection methods on a Poly value are the
# PRE-existing Poly-dispatch gap, nothing fiber-specific.

m = Fiber.new do
  Fiber.yield(1, 2)
  :fin
end
v = m.resume
puts v
__END__
1
2
