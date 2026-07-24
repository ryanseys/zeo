# A method whose return type is a concrete scalar (here mrb_int, pinned by the
# `42` branch) but one branch's value is a call that resolves to nil through a
# nil/unresolved receiver. That call is emitted as a poly nil-box
# (`sp_box_nil()`), and returning it raw from an mrb_int function is
# uncompilable C ("returning sp_RbVal from a function with incompatible result
# type mrb_int"). The fix coerces the box at the return slot
# (`sp_poly_to_i(sp_box_nil())`). The nil branch is never executed here -- the
# regression is purely that the method must compile.
module AR
  def self.adapter
    nil
  end
end

class Base
  def self.count(flag)
    if flag
      42
    else
      AR.adapter.count("x")   # nil receiver -> poly nil-box into the mrb_int slot
    end
  end
end

puts Base.count(true).to_s
