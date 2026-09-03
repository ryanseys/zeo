# Before `HirNode::SelfRef` existed, an escaping block could only ever
# capture `self` implicitly via a bare `@ivar` reference --
# `self.method_name` is another way a block needs the same capture (see
# `analyze::captures`'s new `SelfRef` arm). Also exercises implicit-self
# dispatch (`each_num(a, b)`, no receiver) from inside the SAME method
# that constructs the escaping block.

class Collector
  def initialize(tag)
    @tag = tag
  end

  def tag
    @tag
  end

  def each_num(a, b)
    yield a
    yield b
  end

  def run(a, b)
    each_num(a, b) { |n| puts "tag=#{self.tag}:#{n}" }
  end
end

Collector.new("x").run(1, 2)
__END__
tag=x:1
tag=x:2
