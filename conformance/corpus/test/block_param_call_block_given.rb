# A method that captures its block as `&blk` AND calls `block_given?`
# must still invoke the block through the &blk slot. The block_given?
# call makes the method look like a yield-method, which previously
# routed the literal-block call site through the yield-inline path;
# that path renamed the block local to `blk_y<N>` but left the body's
# `blk.call` referencing the original `lv_blk` (undeclared C). The
# fix skips inlining when the method has a trailing &block param and
# forwards the literal block into that slot instead, padding the
# trailing yield-ABI slots with NULL.

# 1. Top-level &blk + block_given? + blk.call.
def build(&blk)
  blk.call(42) if block_given?
end
build { |x| puts x }              #=> 42

# 2. Expression context: the call result is consumed.
def build_v(&blk)
  r = blk.call(21) if block_given?
  r
end
puts build_v { |x| x * 2 }        #=> 42

# 3. Instance method.
class Foo
  def run(&blk)
    blk.call(7) if block_given?
  end
end
Foo.new.run { |x| puts x }        #=> 7

# 4. Singleton (class) method.
class Bar
  def self.make(&blk)
    blk.call(9) if block_given?
  end
end
Bar.make { |x| puts x }           #=> 9

# 5. A method mixing &blk and an actual yield still works.
def mix(&blk)
  yield 5 if block_given?
end
mix { |x| puts x }                #=> 5

puts "done"
