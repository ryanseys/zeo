# Ruby distinguishes a never-assigned ivar from one assigned `nil`: both READ
# as nil, but only the second is `defined?` and only the second appears in
# `instance_variables`. `@x = {} unless defined? @x` -- observer's idiom --
# depends on it.
class Slot
  def probe
    before = [defined?(@a), instance_variables]
    @a = nil
    middle = [defined?(@a), instance_variables]
    @a = 1
    [before, middle, [defined?(@a), instance_variables]]
  end

  def lazy
    @memo = [] unless defined? @memo
    @memo << :x
    @memo
  end

  def drop
    @gone = 1
    removed = remove_instance_variable(:@gone)
    [removed, defined?(@gone), @gone]
  end
end

p Slot.new.probe
p Slot.new.lazy
p Slot.new.drop

begin
  Slot.new.send(:remove_instance_variable, :@never)
rescue NameError => e
  puts e.message
end

# A fresh object has no ivars at all, so `inspect` shows none.
class Empty
  def initialize(set) = (@v = 1 if set)
end
p Empty.new(false).instance_variables
p Empty.new(true).instance_variables
p Empty.new(false).instance_variable_get(:@v)
p Empty.new(false).instance_variable_defined?(:@v)
p Empty.new(true).instance_variable_defined?(:@v)

# ...and dup/clone carry only what was actually assigned.
p Empty.new(false).dup.instance_variables
p Empty.new(true).clone.instance_variables

# An ivar invented at runtime behaves the same way.
o = Empty.new(false)
p o.instance_variable_defined?(:@invented)
o.instance_variable_set(:@invented, 7)
p o.instance_variables, o.instance_variable_defined?(:@invented)
__END__
[[nil, []], ["instance-variable", [:@a]], ["instance-variable", [:@a]]]
[:x]
[1, nil, nil]
instance variable @never not defined
[]
[:@v]
nil
false
true
[]
[:@v]
false
[:@invented]
true
