# `infer_call_type`'s class-body fallback used to short-circuit on
# any call inside a class body — including those with a receiver.
# A `recv.method(...)` whose method isn't on the enclosing class
# resolved as the default `int`, and the receiver-aware branches
# below (Fiber.yield → poly, infer_open_class_type → user open-
# class return) never ran. The fix gates the fallback on
# `recv < 0`.
#
# Reproducer here: an open-class method on Integer that returns a
# hash. Without the fix, `C#wrap`'s body calls `x.to_pair` whose
# inferred return came out as `int` (the enclosing C class has no
# `to_pair`), but the actual emit returns `sp_SymIntHash *` →
# "returning sp_SymIntHash * from a function with return type
# mrb_int" -Wint-conversion error.

class Integer
  def to_pair
    {a: self, b: self * 2, c: self * 3}
  end
end

class C
  def wrap(x)
    x.to_pair
  end
end

h = C.new.wrap(5)
puts h[:a]   # 5
puts h[:b]   # 10
puts h[:c]   # 15
