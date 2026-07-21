# Bundled tests:
#   - issue219_aref_chain
#   - issue235_chain_tail_widening
#   - ivar_and_write_basic
#   - ivar_c_keyword
#   - ivar_float_array

# === issue219_aref_chain ===
# Issue #219: chained calls on a user-defined `[]` operator's
# result silently lost their coercion (`.to_i` / `.length`) because
# infer_type's `mname == "[]"` branch handled built-in collection
# receivers but fell through to "int" for obj receivers.
# `bag[:id].to_i` then routed through compile_int_method_expr's
# `to_i` (which is identity) and the underlying `const char *`
# leaked through.

class T_issue219_aref_chain_Bag
  def initialize(h); @h = h; end
  def [](k); @h[k.to_sym]; end
end

bag = T_issue219_aref_chain_Bag.new({id: "42"})

# .to_i: infers as int via String#to_i
puts bag[:id].to_i              # 42
puts bag[:id].to_i + 1          # 43

# .length: routes through String#length
puts bag[:id].length            # 2

# Chained into a method expecting mrb_int
class T_issue219_aref_chain_M
  def self.find(id); id + 1; end
end
puts T_issue219_aref_chain_M.find(bag[:id].to_i)      # 43

# Integer-valued bag — .to_s works the other direction (already
# worked before the fix; pinning regression).
class T_issue219_aref_chain_IntBag
  def initialize(h); @h = h; end
  def [](k); @h[k]; end
end
ib = T_issue219_aref_chain_IntBag.new({n: 42})
puts ib[:n].to_s                # "42"

# === issue235_chain_tail_widening ===
# Issue #235 follow-up to #234: chained `@a = @b = expr` only widened
# the chain *head*. The tail (and intermediate participants in
# longer chains) relied on scan_ivars's dual-definite-literal gate,
# which fires for literal RHS but not for CallNode/expression RHS.
# A `@a = @b = make_int` with `@a`/`@b` previously string-typed left
# `@b`'s slot at `const char *` while compile_chained_ivar_writes
# emitted `iv_b = _t1;` with `_t1 : mrb_int`. Same T_issue235_chain_tail_widening_C error shape
# #234 was fixing for the head, just on the tail.

class T_issue235_chain_tail_widening_C
  def initialize
    @a = "hello"
    @b = "world"
  end

  def make_int
    42
  end

  # CallNode RHS — bypasses scan_ivars's literal-gate widening so
  # the chain-drill path is the only thing that can widen `@b`.
  def reset_call
    @a = @b = make_int
  end

  # 3-chain with CallNode RHS — every intermediate must widen.
  def reset_three(c)
    @a = @b = @c = c.length
  end

  def show
    puts @a
    puts @b
  end
end

c = T_issue235_chain_tail_widening_C.new
c.reset_call
c.show                  # 42 / 42

c2 = T_issue235_chain_tail_widening_C.new
c2.reset_three("ab")
c2.show                 # 2 / 2

# 3-chain that adds an `@c` slot to the picture too. After
# reset_three sets @c, calling show wouldn't read it (no accessor),
# but the struct emit needs to type-check.
puts "ok"

# === ivar_and_write_basic ===
class T_ivar_and_write_basic_T
  def initialize
    @ready = false
    @count = 0
  end

  def maybe_inc
    @ready &&= step
  end

  def arm
    @ready = true
  end

  def step
    @count += 1
    true
  end

  def state
    "ready=#{@ready} count=#{@count}"
  end
end

t = T_ivar_and_write_basic_T.new
puts t.state
t.maybe_inc
puts t.state
t.arm
puts t.state
t.maybe_inc
puts t.state
t.maybe_inc
puts t.state

# === ivar_c_keyword ===
# Test: ivar naming remains safe for C keywords and iv_ prefix collisions.

class T_ivar_c_keyword_KeywordIvar
  def initialize
    @if = 40
    @iv_if = 1
  end

  def value
    @if + @iv_if + 1
  end
end

puts T_ivar_c_keyword_KeywordIvar.new.value

# === ivar_float_array ===
# Regression: instance variables initialized via Array.new(n, FILL) must be
# typed by inspecting the fill argument, not always returned as "int_array".
#
# infer_ivar_init_type's CallNode/"new" branch used to unconditionally return
# "int_array" for Array.new(...). That mistyped class fields as the
# containing class's pointer; pointer-type fills additionally lost their GC
# scan function, which would let live elements be swept.
#
# We now check the fill type and use the appropriate typed array container
# (FloatArray for float fills, StrArray for string, sym_array (IntArray
# internally) for symbol, PtrArray for object/pointer fills).
#
# Use float values whose fractional part is non-zero so Spinel's float-puts
# matches CRuby's.

class T_ivar_float_array_Box
  attr_accessor :nums
  def initialize
    @nums = Array.new(3, 0.5)
  end
end

class T_ivar_float_array_SymHolder
  attr_accessor :tags
  def initialize
    @tags = Array.new(2, :alpha)
  end
end

class T_ivar_float_array_Marker
  attr_accessor :id
  def initialize(id)
    @id = id
  end
end

class T_ivar_float_array_ObjHolder
  attr_accessor :marks
  def initialize
    @marks = Array.new(3, T_ivar_float_array_Marker.new(42))
  end
end

b = T_ivar_float_array_Box.new
puts b.nums[0]      # 0.5
puts b.nums[1]      # 0.5
puts b.nums[2]      # 0.5
puts b.nums.length  # 3

s = T_ivar_float_array_SymHolder.new
puts s.tags[0]      # alpha
puts s.tags[1]      # alpha
puts s.tags.length  # 2

m = T_ivar_float_array_ObjHolder.new
puts m.marks[0].id  # 42
puts m.marks[1].id  # 42  (Array.new(n, obj) shares the same obj)
puts m.marks[2].id  # 42
puts m.marks.length # 3

