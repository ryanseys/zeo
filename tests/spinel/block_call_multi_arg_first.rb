# `first` on an Enumerable whose #each invokes the block through
# `block.call(a, b)` with MULTIPLE arguments failed to compile:
#
#   error: cannot convert to a pointer type
#
# `first` takes the block's value as one object, so the multi-argument call has
# to pack its arguments into an array -- the same packing `to_a` needs. The
# `yield a, b` spelling of the same method compiles and answers [1, 2].
#
# Found porting tobi/try, whose fuzzy matcher forwards the caller's block from
# #each and yields (entry, positions, score).
class ViaCall
  include Enumerable
  def each(&block)
    block.call(1, 2)
    block.call(3, 4)
  end
end

class ViaYield
  include Enumerable
  def each
    yield 1, 2
    yield 3, 4
  end
end

p ViaCall.new.first
p ViaYield.new.first
