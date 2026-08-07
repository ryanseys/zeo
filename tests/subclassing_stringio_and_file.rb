# A user subclass of a builtin carries the builtin's native object as a
# payload. `StringIO` and `File` join Array/String/Hash/StringScanner as
# payload roots: puma's `IOBuffer < StringIO` and aws-sdk's `ManagedFile <
# File` are between them the whole aws-sdk-* family's blocker.
#
# `File` is the first root whose own methods are mostly INHERITED -- `read`
# lives on `IO` -- so the payload bridge follows the root's builtin
# superclasses, and stops before Object (`Buffer.new.class` is `Buffer`, not
# `StringIO`).

require "stringio"
require "tmpdir"
require "set"

class Buffer < StringIO
  def dump = "<#{string}>"
end

b = Buffer.new("hello")
p b.dump
p b.read
p [b.is_a?(StringIO), b.class, Buffer.superclass]

class Managed < File
  def open? = !closed?
end

path = File.join(Dir.tmpdir, "zeo_value_subclass_example.txt")
File.write(path, "data")
f = Managed.new(path)
p [f.open?, f.read, f.class]
f.close
p f.open?
File.unlink(path)

class Tags < Set
  def label = "tags(#{size})"
end

t = Tags.new([1, 2, 2])
t << 3
p [t.to_a.sort, t.label, t.class, t.is_a?(Set)]

# `Enumerator`: the payload is the enumerator handle, and like `File` there is
# no empty form -- the block IS the sequence -- so a subclass with its own
# `initialize` seats one through `super() { |y| ... }`. cucumber-messages'
# `NdjsonToMessageEnumerator` is that shape; the eighteen aws-sdk `EventStream`
# classes are the other one, adding a method to the inherited behaviour.
class Ndjson < Enumerator
  def initialize(lines)
    super() do |y|
      lines.each { |l| y.yield(l.upcase) }
    end
  end
end

n = Ndjson.new(["a", "b"])
p [n.class, n.to_a, n.is_a?(Enumerator), n.next]

class EventStream < Enumerator
  def event_types = [:records, :stats]
end

s = EventStream.new { |y| y.yield 1; y.yield 2 }
# `map` builds a NEW Array, so it demotes; `to_a` does too. The receiver keeps
# its own class either way.
p [s.class, s.event_types, s.to_a, s.map { |x| x * 10 }]

# CLASS methods inherited from the payload root. Nothing copies a builtin's
# class-method table rows onto a subclass entry the way materialization copies
# a user `def self.x`, so without the root probe these are invisible --
# rubyzip's `DOSTime.from_time` calls the inherited `local` directly.
class DOSTime < Time
  def self.from_time(t)
    local(t.year, t.month, t.day, t.hour, t.min, t.sec)
  end

  def to_binary_dos_date = day + (month << 5) + ((year - 1980) << 9)
end

d = DOSTime.utc(2024, 5, 6, 7, 8, 9)
# CRuby allocates through the RECEIVER class, so every one of these is a
# DOSTime, not a Time.
p [d.class, d.year, d.month, d.day, d.to_binary_dos_date, d.utc?]
p [DOSTime.now.class, DOSTime.at(0).class, DOSTime.from_time(Time.utc(1999, 3, 4)).class]
p [DOSTime.respond_to?(:local), DOSTime.respond_to?(:nope)]
# `_load` is a PRIVATE class method of Time, so it stays unreachable here --
# exactly as an inherited private class method is in CRuby.
p DOSTime.respond_to?(:_load)
# The conversions are the exception: CRuby answers with the BASE class even
# when the receiver is a subclass.
class Widen < Array; end
p [Widen[1, 2].class, Widen.try_convert([1]).class]

class Stamped < Time
  def initialize(*args)
    super
    @tag = "s"
  end

  attr_reader :tag
end

s = Stamped.new(2000, 1, 2)
p [s.class, s.year, s.month, s.day, s.tag]

# `Thread`: the `File` shape again -- a blockless thread cannot be built, so
# a subclass seats the real one through `super`, which is exactly why
# newrelic_rpm and celluloid subclass it.
class Traced < Thread
  attr_accessor :busy

  def initialize(*args, &block)
    @busy = false
    super(*args) { |*a| block.call(*a) }
  end

  def celluloid? = true
end

t = Traced.new(3) { |n| n * 2 }
p [t.class, t.value, t.celluloid?, t.is_a?(Thread)]
t.busy = true
p t.busy

# Which inherited class methods re-tag as the subclass is a PER-METHOD fact,
# not "the result is a root value": `Thread.current` hands back a thread that
# already exists, and `Enumerator.produce` builds a plain Enumerator, while
# `Time.now` and `Array[]` allocate through the receiver. All four are CRuby's
# own answers.
p [Traced.current.class, Thread.current.class]

class Seq < Enumerator; end
p [Seq.produce(1) { |x| x + 1 }.first(2), Seq.produce(1) { |x| x + 1 }.class]

class Bag < Hash; end
p [Bag[[[1, 2]]].class, Bag.try_convert({}).class]
