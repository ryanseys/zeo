# Bundled tests:
#   - user_cmp_int_ivars
#   - user_method_named_like_mutator
#   - value_type_constructor_arg_gc_root
#   - value_type_ctor_gc_save
#   - value_type_ivar_gc_scan

# === user_cmp_int_ivars ===
# Issue #399: a user-defined `def <=>(other)` whose body calls
# `<=>` on Integer ivars (`@n <=> other.n`) lowered to a recursive
# call to the user's own `<=>` method (passing the int receivers
# cast to `sp_V *`). Type error + would-be infinite recursion.
#
# Fix: compile_int_method_expr now has a `<=>` arm that emits the
# standard 3-way compare for int-vs-int (and int-vs-float). Plus
# `<=>` is added to is_primitive_shared_method so the int-recv
# fallback at compile_call_expr's tail doesn't pick a user class.

class T_user_cmp_int_ivars_V
  attr_reader :n
  def initialize(n)
    @n = n
  end

  def <=>(other)
    @n <=> other.n
  end
end

a = T_user_cmp_int_ivars_V.new(1)
b = T_user_cmp_int_ivars_V.new(2)
c = T_user_cmp_int_ivars_V.new(2)

puts (a <=> b).to_s   # -1
puts (b <=> a).to_s   # 1
puts (b <=> c).to_s   # 0

# Standalone `==` falls back to identity, which doesn't help here;
# the issue is just the recursion inside `<=>`. Spinel's Comparable
# include path is a separate feature.

# === user_method_named_like_mutator ===
# compile_body_return treats certain method names (`update`, `clear`,
# `concat`, `delete`, `each`, `pop`, `push`, `merge!`, ...) as
# statement-only because Hash and Array mutators are conventionally
# called for side-effect. That's right when the receiver IS a Hash
# or Array, but it silently throws away the value when a user class
# happens to define a method with one of those names — including the
# implicit `self.update(...)` form, since the receiver chain isn't
# even consulted before the name match fires.
#
# Pre-fix, `T_user_method_named_like_mutator_C.new.f(7)` returned 0 (correct: 8) and
# `T_user_method_named_like_mutator_D.new.run(T_user_method_named_like_mutator_C.new)` returned 0 (correct: 30). Both follow Ruby
# semantics now.

class T_user_method_named_like_mutator_C
  def update(target)
    target
  end
  def f(n)
    update(n + 1)   # implicit self.update — was discarded
  end
end

class T_user_method_named_like_mutator_D
  def run(c)
    c.update(30)    # explicit recv on user class — was discarded
  end
end

puts T_user_method_named_like_mutator_C.new.f(7)
puts T_user_method_named_like_mutator_D.new.run(T_user_method_named_like_mutator_C.new)

# === value_type_constructor_arg_gc_root ===
class T_value_type_constructor_arg_gc_root_NameTag
  attr_reader :present, :text

  def initialize(present, text)
    @present = present
    @text = text
  end
end

class T_value_type_constructor_arg_gc_root_Payload
end

class T_value_type_constructor_arg_gc_root_Wrapper
  attr_reader :tag, :payload

  def initialize(tag, payload)
    @tag = tag
    @payload = payload
  end

  def self.for_tag(tag)
    junk = []
    i = 0
    while i < 50
      junk << "junk" + i.to_s
      i = i + 1
    end

    T_value_type_constructor_arg_gc_root_Wrapper.new(tag, T_value_type_constructor_arg_gc_root_Payload.new)
  end
end

wrapper = nil
i = 0
while i < 2000
  wrapper = T_value_type_constructor_arg_gc_root_Wrapper.for_tag(T_value_type_constructor_arg_gc_root_NameTag.new(1, "label" + i.to_s))
  i = i + 1
end

puts wrapper.tag.text

# === value_type_ctor_gc_save ===
# A value-type constructor body that introduces a GC-managed local
# (here `arr`, an int_array) needs `SP_GC_SAVE()` paired with the
# `SP_GC_ROOT(lv_arr)` that `declare_method_locals` emits.
#
# `emit_constructor` only emits SP_GC_SAVE for the non-value-type
# branch. For value types it leaves `@in_gc_scope` whatever the
# *previous* method left it as. If the previous method was a
# non-value class's body (which sets `@in_gc_scope = 1`), the
# inherited scope made `declare_method_locals` skip its SP_GC_SAVE
# and the value-type ctor emitted an unbalanced SP_GC_ROOT —
# pushing a stack pointer that becomes dangling on return.
#
# Reproducer: T_value_type_ctor_gc_save_Foo (heap class) is compiled before T_value_type_ctor_gc_save_Vec (value type),
# so without the fix Vec_new's lv_arr root gets pushed without a
# matching save. The fix resets `@in_gc_scope = 0` at the value-
# type ctor entry so declare_method_locals emits SP_GC_SAVE.

class T_value_type_ctor_gc_save_Foo
  def make
    [1, 2, 3]
  end
end

class T_value_type_ctor_gc_save_Vec
  attr_reader :sum
  def initialize(x, y)
    arr = [x, y, x + y]
    @sum = arr[2]
  end
end

f = T_value_type_ctor_gc_save_Foo.new
puts f.make.length            # 3
v = T_value_type_ctor_gc_save_Vec.new(3, 4)
puts v.sum                    # 7

# Many ctor calls so the unbalanced ROOTs would saturate
# sp_gc_nroots if they weren't matched by SP_GC_RESTORE.
sum = 0
i = 0
while i < 200
  vv = T_value_type_ctor_gc_save_Vec.new(i, i + 1)
  sum = sum + vv.sum
  i = i + 1
end
puts sum                       # 200 * (i + (i+1)) summed = sum of (2i+1) for i=0..199 = 200*200 = 40000

# === value_type_ivar_gc_scan ===
class T_value_type_ivar_gc_scan_NameTag
  attr_reader :text

  def initialize(text)
    @text = text
  end
end

class T_value_type_ivar_gc_scan_Payload
end

class T_value_type_ivar_gc_scan_Wrapper
  attr_reader :tag, :payload

  def initialize(tag, payload)
    @tag = tag
    @payload = payload
  end
end

wrappers = []
i = 0
while i < 2000
  wrappers << T_value_type_ivar_gc_scan_Wrapper.new(T_value_type_ivar_gc_scan_NameTag.new("label" + i.to_s), T_value_type_ivar_gc_scan_Payload.new)
  i = i + 1
end

junk = []
i = 0
while i < 5000
  junk << "garbage" + i.to_s
  i = i + 1
end

puts wrappers[1999].tag.text

