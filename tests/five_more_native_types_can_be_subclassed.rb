# A subclass of an instantiable native builtin is one husk wrapping the real
# native value as a PAYLOAD, with the subclass's own ivars beside it. Five more
# builtins join that list, each because a gem subclasses it for the payload.
#
# `Pathname` is the one worth naming: it is pure Ruby over a `@path` ivar in
# CRuby, so it looks like it should need nothing at all -- but zeo reimplements
# it natively, so it needs the payload treatment like the rest.
require "pathname"
require "monitor"
require "tmpdir"

# Pathname: the payload is the path, and `Pathname.new("")` is a real empty
# form, so a subclass without its own `initialize` seeds straight from the root.
class Plain < Pathname
end
p [Plain.new("/tmp/x").to_s, Plain.new("/tmp/x").basename.to_s]
p Plain.new("/a/b").is_a?(Pathname)

class Tagged < Pathname
  attr_reader :tag
  def initialize(path, tag)
    super(path)
    @tag = tag
  end
  def describe = "#{tag}:#{self}"
end
t = Tagged.new("/var/log", :logs)
p [t.describe, t.tag, t.class.to_s]
p [(t + "app.log").to_s, t.absolute?, t.each_filename.to_a]

# It is a Pathname wherever one is expected, including as an ARGUMENT -- which
# is the half a payload husk gets wrong if the comparison only downcasts.
p [File.basename(t), File.join(t, "app.log"), t.to_path]
p Pathname.new("/var/log") == t
p t == Pathname.new("/var/log")

# Monitor: the payload is the lock. This is the shape a gem reaches for when it
# wants a lock bolted onto state of its own.
class Registry < Monitor
  def initialize
    super
    @items = []
  end
  def add(x) = synchronize { @items << x; @items.size }
  def items = synchronize { @items.dup }
end
r = Registry.new
p [r.add(:a), r.add(:b), r.items]
p [r.is_a?(Monitor), r.mon_owned?]

# Mutex: the same, with an explicit `super()`.
class Guard < Mutex
  attr_reader :name
  def initialize(name)
    super()
    @name = name
  end
end
g = Guard.new("db")
p [g.name, g.locked?]
g.lock
p g.locked?
g.unlock
p [g.synchronize { :inside }, g.class.to_s]

# SizedQueue: no maximum can be invented, so the real queue is seated through
# `super(max)` -- and a subclass that skipped it would fail loudly rather than
# hold a queue sized by the compiler.
class Bounded < SizedQueue
  def initialize(max)
    super(max)
    @seen = 0
  end
  def <<(x)
    @seen += 1
    super
  end
  attr_reader :seen
end
b = Bounded.new(2)
b << 1
b << 2
p [b.size, b.max, b.seen, b.pop, b.class.to_s]

# Dir: a directory that exists is needed, so this is the same seated-through-
# super shape.
class Listing < Dir
  attr_reader :note
  def initialize(path, note)
    super(path)
    @note = note
  end
end
d = Listing.new(Dir.tmpdir, "tmp")
p [d.note, d.class.to_s, d.path == Dir.tmpdir]
d.close
