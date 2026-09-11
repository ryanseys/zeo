# A BasicObject descendant has no Kernel#inspect or #to_s. `p`, `puts`,
# interpolation and Array#inspect send those names to it, so a
# method_missing answers them, and without one they raise NoMethodError.
class Tok < BasicObject
  def initialize(array)
    @array = array
  end

  def method_missing(name, ...)
    @array.__send__(name, ...)
  end

  def respond_to_missing?(name, include_private = false)
    @array.respond_to?(name, include_private)
  end
end

t = Tok.new([1, :a])
p t
p [t]
puts t.inspect
print t, "\n"
puts "#{t}"

class Bare < BasicObject; end
b = Bare.new
begin; p b; rescue NoMethodError => e; puts e.message; end
begin; puts b; rescue NoMethodError => e; puts e.message; end
begin; puts [b].inspect; rescue NoMethodError => e; puts e.message; end
begin; "#{b}"; rescue NoMethodError => e; puts e.message; end

class Own < BasicObject
  def inspect = "Own!"
  def to_s = "own"
end
p Own.new
puts Own.new
p [Own.new]
__END__
[1, :a]
[[1, :a]]
[1, :a]
[1, :a]
[1, :a]
undefined method 'inspect' for an instance of Bare
undefined method 'to_s' for an instance of Bare
undefined method 'inspect' for an instance of Bare
undefined method 'to_s' for an instance of Bare
Own!
own
[Own!]
