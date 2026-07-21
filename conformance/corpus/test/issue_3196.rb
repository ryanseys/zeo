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
