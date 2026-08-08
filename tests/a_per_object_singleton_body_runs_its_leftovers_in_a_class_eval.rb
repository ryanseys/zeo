# `class << obj` had a fixed list of statements it could map, and rejected the
# rest. The rejected shape is the same one `class << self` had: a statement
# whose BLOCK reaches for `self`, which no receiver rewrite can rebind.
#
# `class << self` gets a compile-time class for this. A PER-OBJECT singleton
# has none -- so the escape is the runtime one, `recv.singleton_class.class_eval`,
# whose `self` IS that class. That is ruby's own primitive, not a paraphrase.
obj = Object.new
class << obj
  [:a, :b].each do |name|
    define_method(name) { "runtime:#{name}" }
  end
  def plain = :plain
end
p obj.a
p obj.b
p obj.plain

# Reflection cannot tell the two apart, and they keep source order.
p obj.singleton_methods.sort
p obj.singleton_class.instance_methods(false).sort

# A statement that only NAMES `self` needs no runtime body at all -- the `self`
# just has to evaluate to the singleton class. google_drive spells its
# `get_singleton_class` helper exactly this way.
def get_singleton_class(o)
  class << o
    return self
  end
end
x = Object.new
p get_singleton_class(x) == x.singleton_class

# `class << Const` on a CLASS installs class methods, and a statically compiled
# `def self.x` beside it survives.
module Enc
  def self.encode(v) = "enc:#{v}"
end
class Null
  class << self
    def passthru(v) = v
  end
end
class << Null
  Enc.singleton_methods.each do |m|
    define_method(m, Enc.method(m).to_proc)
  end
end
p Null.encode("q")
p Null.passthru(:kept)

# The block's `self` is the singleton class, so a receiverless send inside it
# targets the singleton too -- treetop's `included_modules - Object.included_modules`
# is 101 corpus rows of this.
module Marker; end
target = Object.new
target.extend(Marker)
class << target
  (included_modules - Object.included_modules).each do |m|
    define_method(:"from_#{m}") { m.to_s }
  end
end
p target.from_Marker

# A statement with no `self` under it still runs unchanged, at its position.
$order = []
sink = Object.new
class << sink
  $order << :before
  [:late].each { |n| define_method(n) { n } }
  $order << :after
end
p $order
p sink.late
