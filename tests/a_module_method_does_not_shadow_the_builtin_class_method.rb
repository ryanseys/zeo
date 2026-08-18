# Reopening a builtin MODULE and defining a method the including CLASS
# already defines itself must not shadow the class's own method. `Array`
# includes `Enumerable`, and `Array#none?`/`Array#size` are defined on `Array`
# -- so `Array` comes first in the ancestry and its own definitions win.
# CRuby answers `true` / `3`; zeo answers `:from_enumerable` /
# `:from_enumerable_size`, taking the module's.
#
# The cause is which side of the MRO the BUILTIN rows sit on. A user's `def`
# inside `module Enumerable` registers as an `own_method` on the Enumerable
# ClassInfo, and `mro::materialize_methods` walks the ancestry putting the
# closest ancestor's own_methods first -- Array, then Enumerable -- so the
# table itself is ordered correctly. What is missing is `Array`'s own row:
# `none?` and `size` are Rust builtins with no `Scope`, so `Array` contributes
# nothing to `own_methods` for those names and the walk falls through to the
# first ancestor that DOES have a user scope. The builtin table is consulted
# only after the materialized table misses.
#
# The fix is for materialization to treat a builtin row on `class_id` as
# claiming the name -- the same `seen.insert` an `undef` already performs --
# so a further ancestor's user definition cannot supply it. That needs the
# builtin method list per class at materialization time, which is
# `zeo_abi::BUILTINS` and is already indexed by ClassId.
#
# Independent of the fused-iterator work: this reproduces byte-identically on
# the binary built before Wave 8's kinds existed, with the site unfused.
module Enumerable
  def none?
    :from_enumerable
  end

  def size
    :from_enumerable_size
  end
end

arr = [1, 2, 3]
p arr.none? { |x| x > 9 }
p arr.size
p [].none?
