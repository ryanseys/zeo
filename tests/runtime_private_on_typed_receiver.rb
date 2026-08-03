# Visibility is a RUNTIME property in ruby: `private :name` re-marks a method
# that already exists, and every later call sees the new mark.
#
# A call whose receiver class zeo knows at compile time is a direct call to the
# generated function, and it decided visibility once -- when it was emitted. So
# the names a runtime visibility verb could re-mark join the same
# `Compiler::runtime_patches` set a runtime `define_method` feeds, and the fold
# stands down for them. The dynamic path already asked the runtime per call.
#
# A class-body `private :m` is untouched: lowering retags the `def` in place,
# so no call survives for the scan to see.

class Late
  def open_now = :open
  def stays = :stays
end

holder = Late.new
p holder.open_now

Late.send(:private, :open_now)

begin
  p holder.open_now
rescue NoMethodError => e
  puts e.message
end

# Reachable without an explicit receiver, and through `send`, exactly as before.
class Late
  def reach = open_now
end
p holder.reach
p holder.send(:open_now)

# ...and promotable again.
Late.send(:public, :open_now)
p holder.open_now

# `protected` is the third state, and takes a LIST.
class Pair
  def a = 1
  def b = 2
  def c = 3
  def sum(other) = other.a + other.b
end
x = Pair.new
y = Pair.new
p x.sum(y)
Pair.send(:protected, :a, :b)
p x.sum(y)
begin
  x.a
rescue NoMethodError => e
  puts e.message
end
p x.c

# The class-method twin.
class Fact
  def self.build = :built
  def self.keep = :kept
end
p Fact.build
Fact.send(:private_class_method, :build)
begin
  Fact.build
rescue NoMethodError => e
  puts e.message
end
p Fact.keep

# A class body's own `private` still folds -- nothing about it is deferred.
class Body
  def shown = :shown
  def hidden = :hidden
  private :hidden
end
p Body.new.shown
begin
  Body.new.hidden
rescue NoMethodError => e
  puts e.message
end
p Body.private_instance_methods(false)
