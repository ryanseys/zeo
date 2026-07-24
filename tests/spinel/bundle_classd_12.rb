# Bundled tests:
#   - multi_write_map_block
#   - multi_write_setter
#   - multi_write_typed_array_dispatch
#   - mutable_str_into_poly_slot
#   - nested_class_in_class

# === multi_write_map_block ===
# `A, B = arr.map { block }` is destructuring assignment where
# each LHS target receives one element of the mapped array.
# Two related codegen pieces need to land together:
#
# 1. Type inference for the target slot. When the block returns a
#    typed array (e.g. int_array), the target must be typed
#    accordingly. Without the fix, the outer `infer_type` collapses
#    array-of-array to the int_array placeholder and every target
#    comes out as int.
#
# 2. Code emission for constant targets when the RHS evaluates to
#    a ptr_array. Without the fix, the multi-write codegen had
#    branches for ArrayNode literal RHS and int_array RHS but no
#    ptr_array branch — every constant target stayed unset.

class T_multi_write_map_block_C
  # Outer map produces a ptr_array (each element is an int_array
  # produced by the inner map).
  P, Q = [3, 5].map { |n| (0...n).to_a }
end

# Without the inference fix, P and Q would be typed `int` and the
# `.length` / `[]` calls below wouldn't compile against the real
# `sp_IntArray *` values stored in them.
puts T_multi_write_map_block_C::P.length   # 3
puts T_multi_write_map_block_C::P[0]       # 0
puts T_multi_write_map_block_C::P[1]       # 1
puts T_multi_write_map_block_C::P[2]       # 2
puts T_multi_write_map_block_C::Q.length   # 5
puts T_multi_write_map_block_C::Q[3]       # 3
puts T_multi_write_map_block_C::Q[4]       # 4

# === multi_write_setter ===
# `a, b, c.x = expr` where one target is a setter call (`c.x =`)
# parses to a CallTargetNode in the targets list. Without parser
# support for CallTargetNode the setter target was silently dropped.

class T_multi_write_setter_Box
  def initialize; @v = 0; end
  attr_accessor :v
end

class T_multi_write_setter_Holder
  def initialize
    @a = 0
    @b = 0
    @c = T_multi_write_setter_Box.new
  end
  def fill
    @a, @b, @c.v = [11, 22, 33]
  end
  def show
    puts @a
    puts @b
    puts @c.v
  end
end

h = T_multi_write_setter_Holder.new
h.fill
h.show

# === multi_write_typed_array_dispatch ===
# Regression: `a, b = self.method(...)` where `method` returns a typed
# array other than int_array. compile_multi_write previously emitted
# `sp_IntArray *tmp = call(); sp_IntArray_get(tmp, k)` for every
# array-returning RHS that wasn't poly_array / poly. A method
# returning e.g. obj_<C>_ptr_array therefore type-mismatched at the
# C-temp declaration and dispatched the wrong getter.
#
# Each shape below has the call return a typed array and destructures
# with two locals; the values must round-trip cleanly.

class T_multi_write_typed_array_dispatch_Mat
  attr_accessor :v
  def initialize(v); @v = v; end
end

class T_multi_write_typed_array_dispatch_N
  def two_mats
    [T_multi_write_typed_array_dispatch_Mat.new(11), T_multi_write_typed_array_dispatch_Mat.new(22)]
  end
  def two_floats
    [1.5, 2.5]
  end
  def two_strs
    ["a", "b"]
  end

  def go_mats
    a, b = self.two_mats
    a.v + b.v
  end
  def go_floats
    a, b = self.two_floats
    a + b
  end
  def go_strs
    a, b = self.two_strs
    a + b
  end
end

n = T_multi_write_typed_array_dispatch_N.new
puts n.go_mats     # 33
puts n.go_floats   # 4.0
puts n.go_strs     # "ab"

# === mutable_str_into_poly_slot ===
# #541. A view function returning sp_String * boxed into a
# `body` slot that had been widened to poly (other classes wrote
# ints/etc. to the same-named slot) emitted
# `sp_box_obj((void *)val, 0)` -- a generic SP_TAG_OBJ box with
# cls_id 0 (BasicObject). Downstream code that dispatches on
# the body's tag/cls_id (e.g. Tep::Server#write_response in
# roundhouse, computing Content-Length and writing the wire)
# had no arm matching SP_TAG_OBJ + cls_id 0, so the body was
# silently dropped -- HTTP 200 with empty body in real-blog.
#
# Fix: `box_non_nullable_value_to_poly`'s mutable_str arm now
# routes through `sp_box_str(s->data)` (SP_TAG_STR with the
# underlying char* buffer) instead of the catch-all
# `sp_box_obj((void *)..., 0)`. Consumers reading `v.v.s` on
# the tag find the string content.

class T_mutable_str_into_poly_slot_View
  def self.index
    s = String.new
    s << "<h1>title</h1>"
    s << "<p>body</p>"
    s
  end
end

class T_mutable_str_into_poly_slot_Resp
  attr_accessor :body
  def initialize; @body = ""; end
end

class T_mutable_str_into_poly_slot_OtherResp
  attr_accessor :body
  def initialize; @body = 42; end  # widens body across classes to poly
end

r = T_mutable_str_into_poly_slot_Resp.new
r.body = T_mutable_str_into_poly_slot_View.index
# Without the fix: body boxed as sp_box_obj((void *), 0); .inspect
# dispatches via sp_poly_inspect which falls into the SP_TAG_OBJ
# case but cls_id 0 matches no arm; result is generic /
# truncated. With the fix: body boxed as SP_TAG_STR; inspect
# returns the quoted string.
puts r.body.inspect

# Trigger the T_mutable_str_into_poly_slot_OtherResp widening so the analyzer sees both
# int and string writes; verifies the str arm still works under
# pre-widened body slots.
o = T_mutable_str_into_poly_slot_OtherResp.new
puts o.body.inspect

r2 = T_mutable_str_into_poly_slot_Resp.new
r2.body = T_mutable_str_into_poly_slot_View.index
puts r2.body.inspect

# === nested_class_in_class ===
# Nested class definition inside another class.
# Spinel has no namespace table; bare class names must be unique, so
# `class T_nested_class_in_class_A; class B; ... end; end` registers `B` at top level the same
# way `module M; class B; ... end; end` does.

class T_nested_class_in_class_A
  class B
    def initialize(x)
      @x = x
    end
    attr_reader :x
  end

  def make_b(x)
    B.new(x)
  end
end

# Direct construction via the path
b = T_nested_class_in_class_A::B.new(7)
puts b.x

# Construction via the outer class
b2 = T_nested_class_in_class_A.new.make_b(42)
puts b2.x

# T_nested_class_in_class_A nested class with its own nested class
class T_nested_class_in_class_Outer
  class Mid
    class Inner
      def hello
        "hi"
      end
    end
  end
end

puts T_nested_class_in_class_Outer::Mid::Inner.new.hello

