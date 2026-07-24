# Auto-splat: a single array arg destructured across N>=2 block params
# (CRuby binds arr[0]..arr[N-1] at arity >= 2). The params are typed
# from the array's element type and codegen reads each element via the
# typed sp_<Prefix>_get.

class Box
  def initialize(v)
    @v = v
  end

  def base
    @v
  end
end

class BoxPlus < Box
end

b = BoxPlus.new(100)

# Array literal arg, three params, combined with a rebound-self call.
puts(b.instance_exec([1, 2, 3]) { |x, y, z| base + x + y + z })   #=> 106

# Two params; the element type drives the param C types.
puts(b.instance_exec([10, 20]) { |x, y| x + y })                  #=> 30

# String-element array auto-splats too.
puts(b.instance_exec(["a", "b"]) { |x, y| x + y })                #=> ab

# A top-level-local array arg resolves its element type during the
# rewrite walk (the root walk now declares top-level locals in scope),
# so it auto-splats the same as a literal.
nums = [10, 20]
puts(b.instance_exec(nums) { |x, y| base + x + y })               #=> 130

words = ["x", "y"]
puts(b.instance_exec(words) { |x, y| x + y })                     #=> xy
puts "done"
