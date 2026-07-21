# Constants defined in a late-reopened module body resolve from
# instance methods on a class that included the module. Sibling to
# #573 (`b034816 reconcile_class_includes`) which handled the same
# forward-decl + include + late-reopen pattern for methods but not
# for constants.
#
# The fix walks `@cls_includes[@current_class_idx]` in
# `resolve_const_read_name` (both analyzer and codegen sides) so a
# bare `CONST` reference from a method body attached via `include M`
# resolves M::CONST. Issue #591.

module M
end

class C
  include M
end

module M
  CONST = { a: 1, b: 2 }
  OTHER = "hello"

  def check
    puts "len=" + CONST.length.to_s
    puts "other=" + OTHER
  end
end

C.new.check

# Same pattern with nested namespace -- the host class is still
# the search target.
module M2
end

class D
  include M2
end

module M2
  ITEMS = [10, 20, 30]

  def total
    ITEMS.length
  end
end

puts D.new.total                # 3
