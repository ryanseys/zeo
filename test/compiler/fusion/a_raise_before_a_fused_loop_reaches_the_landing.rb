#@ zeo-env: ZEO_RT_LEAKCHECK=1
# The landing block is shared: `Fx::new` creates ONE of it and every
# `Fx::fallible` branches there, so a raise anywhere in a method reaches the
# same epilogue that a normal return does -- and that epilogue releases every
# slot in `fx.locals` unconditionally.
#
# A fused `n.times` splice takes no run-time guard, so it used to look safe.
# It was not: its block parameter's slot was nil-initialised where the splice
# was emitted, and a raise BEFORE the loop reached the landing without ever
# running that block. This pins the path.
#
# Runs under `ZEO_RT_LEAKCHECK=1`; the printed output is the same either way.
#
# HONEST LIMIT: this golden does NOT reproduce the bug on the pre-fix
# emitter -- it was tried, and it came out clean. The released bytes are
# stack garbage, and a short frame leaves benign garbage behind. The proof
# is `clif_snapshot_fused_each_guards` in `crates/zeo/tests/clif.rs`, which
# can see emitter SHAPE. What this file adds is coverage of the path, so a
# LOUD failure on it is caught.

def raises_before_a_loop(obj)
  obj.no_such_method
  3.times { |i| p i }
rescue NoMethodError => e
  e.class
end

def raises_before_a_range_loop(obj)
  obj.no_such_method
  (0...3).each { |i| p i }
rescue NoMethodError => e
  e.class
end

p raises_before_a_loop(Object.new)
p raises_before_a_range_loop(Object.new)
__END__
NoMethodError
NoMethodError
