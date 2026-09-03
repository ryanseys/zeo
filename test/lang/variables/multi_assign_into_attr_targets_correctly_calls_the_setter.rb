# A real, previously-undetected bug: `CallTargetNode::name()` is
# ALREADY the setter name (`:x=`, confirmed via `Prism.parse`), but
# `parse::lower_multi_target` appended ANOTHER `=`, building an
# unresolvable `x==` method name -- so ANY multi-assignment into two
# attr targets (`b.x, b.y = b.y, b.x`, the idiomatic in-place swap)
# panicked with a confusing "unsupported call `x==`" instead of
# swapping the values. Array-index multi-assign targets (`arr[0],
# arr[1] = ...`) were unaffected (a distinct code path, `[]=`, not
# string-formatted from a name at all).

arr = [1, 2, 3, 4]
arr[0], arr[1] = arr[1], arr[0]
puts arr[0]
puts arr[1]

class Box
  attr_accessor :x, :y
  def initialize(x, y)
    @x = x
    @y = y
  end
end
b = Box.new(1, 2)
b.x, b.y = b.y, b.x
puts b.x
puts b.y
__END__
2
1
2
1
