# An accumulator array mutated through an INSTANCE-method parameter and read
# back by element must round-trip its objects intact. The object-array
# narrowing pass built interprocedural edges only for free functions, so an
# instance method's param narrowed to an unboxed sp_PtrArray while every
# caller's local stayed a boxed poly array -- push wrote raw pointers where
# boxed values were read, and an ivar getter through the broken box returned
# 0 (and an :float_array FFI marshal NULL: the toy 72-tensor no-op init).
class Mat2
  attr_accessor :nrows, :flat
  def initialize
    @nrows = 4
    @flat = [1.0, 2.0, 3.0, 4.0]
  end
end

class Eng
  def alloc_w(inits, m)
    inits.push(m)
    m
  end

  def up(m)
    puts "#{m.nrows} #{m.flat.inspect}"
  end

  def go
    inits = []
    alloc_w(inits, Mat2.new)
    alloc_w(inits, Mat2.new)
    gk = 0
    while gk < 2
      up(inits[gk])
      gk = gk + 1
    end
    inits.length
  end
end

e = Eng.new
p e.go
e.up(Mat2.new)   # a concrete local through the same method keeps working
