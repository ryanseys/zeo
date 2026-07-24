# A single splat arg to a *direct* instance_exec spreads its source
# array across the block's params, exactly as passing the array
# directly does (CRuby auto-splat). The receiver stays heap (Box has a
# subclass), and the block reads the rebound self's ivar alongside the
# spread elements. (The implicit-self trampoline splat is covered by
# instance_exec_splat.rb; this is the explicit-receiver direct lift.)
class Box
  def initialize(v)
    @v = v
  end
end

class BoxPlus < Box
end

b = BoxPlus.new(5)

# splat of an int array across two params
args = [10, 7]
puts b.instance_exec(*args) { |a, c| a + c + @v }

# a directly-passed array auto-splats the same way (regression anchor)
puts b.instance_exec([3, 4]) { |a, c| a * c }

# splat of a string array
words = ["x", "y"]
puts b.instance_exec(*words) { |a, c| a + c }

# a sole splat spreads across a single param too: the block's lone param
# binds to the first spread element (arr[0]), not the whole array -- this
# is where a splat diverges from a directly-passed array, and the param
# count alone (1) would otherwise leave the splat un-spread.
puts b.instance_exec(*args) { |a| a + @v }
puts b.instance_exec(*words) { |a| a }
