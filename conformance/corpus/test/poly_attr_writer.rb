# Regression: a setter call (`obj.x = val`) on a parameter typed as
# a class union must execute the assignment. Previously the dispatch
# loop emitted no arms for attr_writer setters on poly receivers and
# silently dropped the store.
#
# Three subcases:
#  1. Single class with `x`, second class without -- the union case
#     where the dispatch arm is "skipped silently."
#  2. Two classes share an ivar name; same value type. Verifies
#     both arms emit and the result temp's C type fits the slot.
#  3. Two classes share an ivar name; differing value types
#     (string vs int). Forces the new arm to box the rhs poly-style
#     for one cls_id and unbox for the other.
#
# Verifying via the *side effect* (the ivar's value) rather than the
# setter expression's return value -- propagating setter returns
# through the dispatch into the enclosing function's return is a
# separate concern that this PR doesn't try to fix.

# --- 1. Single class (the no-op repro) ---------------------------------

class Foo
  attr_accessor :x
  def initialize; @x = 0; end
end

class Bar
  # No `x`; coexists only to widen `obj` to a class union.
end

def set_x(obj, v)
  obj.x = v
end

foo = Foo.new
bar = Bar.new
set_x(foo, 42)
set_x(bar, 99) rescue nil

puts foo.x


# --- 2. Two classes share `body` (string in both) ---------------------

class Req
  attr_accessor :body
  def initialize; @body = ""; end
end

class Res
  attr_accessor :body
  def initialize; @body = ""; end
end

def set_body(o, b)
  o.body = b
end

req = Req.new
res = Res.new
set_body(req, "REQ")
set_body(res, "RES")

puts req.body
puts res.body


# --- 3. Two classes share `slot` with *different* value types ---------
# Pre-fix poly_dispatch_return_type returned "int" for any setter
# whose name didn't appear as an explicit method, so the result temp
# was mrb_int and the string arm's `tmp = lv` mismatched. The new
# arm now also unboxes the rhs to the slot's concrete type when
# the function param widened to poly.

class IBox
  attr_accessor :slot
  def initialize; @slot = 0; end
end

class SBox
  attr_accessor :slot
  def initialize; @slot = ""; end
end

def set_slot(o, v)
  o.slot = v
end

ibox = IBox.new
sbox = SBox.new
set_slot(ibox, 7)
set_slot(sbox, "hello")

puts ibox.slot
puts sbox.slot
