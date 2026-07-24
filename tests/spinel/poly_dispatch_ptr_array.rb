# Polymorphic `[]` and `length` dispatch must reach a PtrArray
# receiver. Pre-fix, two things were wrong:
#
# 1. `emit_poly_builtin_dispatch` had no SP_BUILTIN_PTR_ARRAY branch
#    for `[]` (the in-source comment said "Defer (the issue notes
#    this is more involved)"). The dispatch result silently stayed at
#    its default 0 / NULL when the runtime tag was a PtrArray.
#
# 2. `box_expr_to_poly` checked `is_obj_type` *before*
#    `is_ptr_array_type`. Both predicates fire on `obj_Foo_ptr_array`
#    (it starts with `obj_` AND ends with `_ptr_array`), so the
#    is_obj_type branch ran first and emitted
#    `sp_box_obj(p, find_class_idx("Foo_ptr_array"))`
#    = `sp_box_obj(p, -1)`. -1 happens to equal SP_BUILTIN_INT_ARRAY,
#    so the runtime cls_id mis-tagged a PtrArray as an IntArray.
#    Subsequent dispatch then fell into the IntArray branch and read
#    garbage out of the PtrArray's layout.
#
# `lenof([Foo.new(1), Foo.new(2)])` previously printed 16 (a bogus
# byte interpreted as IntArray length); now it prints 2.

class Foo
  def initialize(n)
    @n = n
  end
  attr_reader :n
end

class Bar
  def length
    99
  end

  def [](_)
    77
  end
end

def lenof(a)
  a.length
end

def first(a)
  a[0]
end

# `length` over mixed call sites — IntArray, PtrArray<Foo>, and a
# user class. The PtrArray branch is the new one for this PR.
puts lenof([1, 2, 3])                  # 3
puts lenof([Foo.new(1), Foo.new(2)])   # 2
puts lenof(Bar.new)                    # 99

# `[]` over the same shapes.
puts first([10, 20, 30])               # 10
foo = first([Foo.new(42)])
puts foo.n                             # 42
puts first(Bar.new)                    # 77
