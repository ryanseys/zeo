# The widened sweep behind `a_later_def_on_a_builtin_reaches_back`: every
# shape a compile-time reopen of a builtin can take, called from ABOVE its own
# `class Foo ... end` and again from below.
#
# A name the builtin never had raises ruby's plain `undefined method` before
# the reopen -- there is no row to forward to, and "no superclass method"
# would name a mechanism the program never wrote. Arguments and blocks reach
# the forwarded row. A reopened MODULE counts too: its rows materialize onto
# every includer, so the flag is keyed by the DEFINING class and every copy
# shares it. A reopen inside a branch that never runs never goes live at all,
# which is exactly right.

def try(label)
  p [label, yield]
rescue NoMethodError, ArgumentError, TypeError => e
  p [label, :raised, e.class.to_s, e.message]
end

# 1. a brand-new name the builtin never had
try(:new_before) { [].brand_new }
class Array
  def brand_new = :brand_new
end
try(:new_after)  { [].brand_new }

# 2. args and blocks forwarded to the native row
try(:map_before) { [1, 2].map { |x| x * 2 } }
try(:pop_before) { a = [1, 2, 3]; [a.pop(2), a] }
try(:push_before){ [1].push(2, 3) }
try(:fetch_kw)   { [1].fetch(9) { |i| [:blk, i] } }
class Array
  def map = :own_map
  def pop(*) = :own_pop
  def push(*) = :own_push
  def fetch(*) = :own_fetch
end
try(:map_after)  { [1, 2].map { |x| x * 2 } }

# 3. a module reopen
try(:mod_before) { [1, 2].each_slice(1).to_a }
module Enumerable
  def each_slice(*) = :own_slice
end
try(:mod_after)  { [1, 2].each_slice(1) }

# 4. a reopen inside a conditional that never runs
try(:cond_before) { "x".upcase }
if false
  class String
    def upcase = :never
  end
end
try(:cond_after)  { "x".upcase }

# 5. a reopen with super where a real super target exists
class Hash
  def keys = [:own, super]
end
try(:hash_keys)   { { a: 1 }.keys }
__END__
[:new_before, :raised, "NoMethodError", "undefined method 'brand_new' for an instance of Array"]
[:new_after, :brand_new]
[:map_before, [2, 4]]
[:pop_before, [[2, 3], [1]]]
[:push_before, [1, 2, 3]]
[:fetch_kw, [:blk, 9]]
[:map_after, :own_map]
[:mod_before, [[1], [2]]]
[:mod_after, :own_slice]
[:cond_before, "X"]
[:cond_after, "X"]
[:hash_keys, :raised, "NoMethodError", "super: no superclass method 'keys' for an instance of Hash"]
