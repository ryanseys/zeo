# initialize calls a method that builds a local array, then taps an object into
# the collection it was handed.
# (spinel issue #3196)
class Flag; end
class Crash
  attr_reader :flags
  def initialize
    @flags = []
    append(["--help"], @flags)
  end
  def append(names, collection)
    available = []
    names.each { available << _1 }
    add(Flag.new).tap { collection << _1 }
  end
  def add(flag) = flag
end
c = Crash.new
p c.flags.size
p c.flags[0].is_a?(Flag)
__END__
1
true
