#@ zeo-env: ZEO_RT_LEAKCHECK=1
# A fused `arr.each` splice is guarded twice at run time -- the receiver's
# tag must be Array, and `zeo_rt_iter_inline_ok_for` must still hold -- and
# the FALLBACK arm is what runs when either fails. That arm reaches the same
# epilogue as the inline one, so every slot the epilogue releases has to be
# initialised on it too.
#
# `Array.send(:define_method, :each)` is what makes the second guard answer
# false, so both arms run here in one program: the first call takes the
# inline arm, the second the fallback. Before slot initialization moved to
# the entry block, the block parameter's slot was nil-initialised INSIDE the
# inline arm, so the second call released a slot holding stack garbage.
# `tests/compiled_code_releases_a_dead_slot.rb` has the mechanism.
#
# A RUN-TIME redefinition, deliberately. A compile-time `class Array; def
# each` reaches backwards and makes the FIRST call answer 30 as well --
# that is `tests/gaps/a_later_def_on_a_builtin_reaches_back.rb`, a separate
# gap this file must not depend on.
#
# Runs under `ZEO_RT_LEAKCHECK=1`, which is what makes it a memory-safety
# test rather than an output test.
#
# HONEST LIMIT: this golden does NOT reproduce the bug on the pre-fix
# emitter -- it was tried, and it came out clean. The released bytes are
# stack garbage, and a short frame leaves benign garbage behind. The proof
# is `clif_snapshot_fused_each_guards` in `crates/zeo/tests/clif.rs`, which
# can see emitter SHAPE. What this file adds is coverage of the path, so a
# LOUD failure on it is caught.

def sum_of(a)
  total = 0
  a.each { |x| total += x }
  total
end

p sum_of([1, 2, 3])

Array.send(:define_method, :each) do |&b|
  b.call(10)
  b.call(20)
  self
end

p sum_of([1, 2, 3])
__END__
6
30
