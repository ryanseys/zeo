# Bundled tests:
#   - issue176_empty_hash_default
#   - issue204_user_find_fetch

# === issue176_empty_hash_default ===
# Issue #176: empty-Hash default parameter `def m(h = {})` used to
# silently corrupt the caller-supplied Hash because unify_call_types
# widened the param type to "poly" and the poly dispatch tree had no
# arm for SymIntHash/StrIntHash. Hash variants now escalate the
# str_int_hash empty default to the call-site type, mirroring the
# int_array → typed-array escalation.

class T_issue176_empty_hash_default_A
  def initialize(attrs = {})
    @x = attrs[:x]
  end
  def x; @x; end
end

puts T_issue176_empty_hash_default_A.new({x: 42}).x

class T_issue176_empty_hash_default_B
  def initialize(attrs = {})
    @name = attrs[:name]
  end
  def name; @name; end
end

puts T_issue176_empty_hash_default_B.new({name: "alice"}).name

# String-keyed hash also works.
class T_issue176_empty_hash_default_C
  def initialize(attrs = {})
    @v = attrs["v"]
  end
  def v; @v; end
end

puts T_issue176_empty_hash_default_C.new({"v" => 7}).v

# === issue204_user_find_fetch ===
# Issue #204: user-defined `find` / `fetch` were overridden by the
# method-name dispatch's "int" fallback even when the receiver
# wasn't a built-in collection. The body's actual return type
# (string here) lost; the call site mistyped the result as int.

# Class method form (canonical ActiveRecord finder shape).
class T_issue204_user_find_fetch_Bag
  def self.find(id); "row-#{id}"; end
  def self.fetch(id); "fetched-#{id}"; end
end
puts T_issue204_user_find_fetch_Bag.find(42)        # row-42
puts T_issue204_user_find_fetch_Bag.fetch(99)       # fetched-99

# Instance method form on a hash-wrapping class.
class T_issue204_user_find_fetch_C
  def initialize(h); @h = h; end
  def find(key); @h[key.to_sym]; end
  def fetch(key); @h[key.to_sym]; end
end
c = T_issue204_user_find_fetch_C.new({a: "alpha", b: "beta"})
puts c.find(:a)          # alpha
puts c.fetch(:b)         # beta

# Built-in collection dispatch still works (regression check).
puts ["x", "y", "z"].find { |v| v == "y" }   # y

