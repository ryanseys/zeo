# each_cons(2).map { |a, b| ... } over an Int array: the flat multi-param window
# binding boxes each scalar element when the param is poly-typed, instead of
# assigning a raw mrb_int into an sp_RbVal slot.
codes = (0...8).map { |n| n * 2 }
d = codes.each_cons(2).map { |a, b| a + b }
w = codes.last
p d
p w
p [1.0, 2.0, 3.0].each_cons(2).map { |a, b| a * b }
