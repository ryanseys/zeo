# Bundled tests:
#   - proc_hash_value
#   - ptr_array_delete_at
#   - reopen_class

# === proc_hash_value ===
# Issue #65: an ivar initialized as `{}` and later assigned `&block`
# captured procs lowered the slot to str_int_hash, then fed
# `sp_Proc *` into `sp_StrIntHash_set` (which expects mrb_int).
#
# Two fixes:
#   - Issue #64's empty-hash promotion now resolves the slot to
#     `str_poly_hash` when the value type is `proc`.
#   - `box_expr_to_poly` / `box_value_to_poly` learned a `proc`
#     branch so the `[]=` site emits `sp_box_proc(...)` instead of
#     falling through to `sp_box_int`.
#   - The empty-hash inline-init paths (compile_stmt and
#     emit_constructor) now route to `sp_StrPolyHash_new()` /
#     `sp_SymPolyHash_new()` for poly-valued promotions.

class T_proc_hash_value_Registry
  def initialize
    @builtins = {}
  end

  def define_builtin(name, &block)
    @builtins[name] = block
  end
end

r = T_proc_hash_value_Registry.new
r.define_builtin("x") { 1 }
puts "ok"

# === ptr_array_delete_at ===
# Regression: `Array#delete_at(i)` on an array of user-class
# instances. Pre-fix the codegen had the dispatch arm only for
# IntArray / StrArray; PtrArray<UserClass> hit the unresolved-call
# warning and emitted 0, so user-class arrays could push but never
# shrink. Hit while building Tep::Scheduler's fiber-slot list:
# the `clear` loop never terminated because `delete_at` was a no-op.

class T_ptr_array_delete_at_Box
  attr_accessor :tag
  def initialize(tag)
    @tag = tag
  end
end

a = [T_ptr_array_delete_at_Box.new("seed")]
a.delete_at(0)
a.push(T_ptr_array_delete_at_Box.new("a"))
a.push(T_ptr_array_delete_at_Box.new("b"))
a.push(T_ptr_array_delete_at_Box.new("c"))
puts a.length.to_s     # 3

# Remove from middle.
v = a.delete_at(1)
puts v.tag             # b
puts a.length.to_s     # 2
puts a[0].tag          # a
puts a[1].tag          # c

# Negative index counts from end.
last = a.delete_at(-1)
puts last.tag          # c
puts a.length.to_s     # 1

# Out-of-range index returns nil-equivalent (NULL); array
# unchanged.
gone = a.delete_at(99)
puts gone.nil? ? "nil" : "not-nil"
puts a.length.to_s     # 1

# Drain via repeated delete_at(0) -- the use case that surfaced
# this bug. Loop must terminate.
while a.length > 0
  a.delete_at(0)
end
puts a.length.to_s     # 0
puts a.empty? ? "empty" : "not-empty"

# === reopen_class ===
# Test: reopening a user-defined class merges into the existing one
# instead of producing duplicate C struct/constructor definitions.

class T_reopen_class_Point
  attr_accessor :x
  def initialize(x, y)
    @x = x
    @y = y
  end
end

class T_reopen_class_Point
  attr_accessor :y
  def to_s
    "(" + @x.to_s + "," + @y.to_s + ")"
  end
end

p = T_reopen_class_Point.new(3, 4)
puts p.to_s
puts p.x
puts p.y
p.x = 10
p.y = 20
puts p.to_s

class T_reopen_class_Foo
end

class T_reopen_class_Foo
end

puts "ok"

