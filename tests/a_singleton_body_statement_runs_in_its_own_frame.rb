# Ruby gives a `class << self` body a backtrace frame of its own, labelled
# `singleton class`, with the enclosing `<class:Config>` frame beneath it.
#
# zeo splices the body's statements into the ENCLOSING class body -- that is
# the whole point of the retagging model, and moving them out was ruled out
# (a class body opens its own LOCAL SCOPE, and a singleton body's locals are
# shared across the whole of it, so `class << self; x = 1; puts x; end` would
# break the moment some items moved and others stayed). So the frame arrives
# without the statements moving: lowering records which `class << self` each
# spliced statement came from, and `codegen::stmt::emit_body` groups
# consecutive ones under one `singleton class` guard.
#
# TWO positions had to be right, not one. The frame itself tracks the group's
# own lines (40, 50 below), and the ENCLOSING frame is left reading the
# `class << self` KEYWORD's line (39, 49) rather than the first grouped
# statement's -- which is why the group stamps that line before pushing the
# guard instead of letting the statement stamp its own underneath.
# `Ctx::lexical_frame_label` carries the label down, so the block in `Boom`
# is `block in singleton class` and not `block in <class:Boom>`.
#
# The first two bodies below are handled IN PLACE -- the case that had no
# frame: `Item::SelfSend` (a receiverless or explicit-`self` call rebound
# onto `self.singleton_class`) for the first, and `Item::Passthrough`
# (anything `mentions_self` says never consults `self`) for the second. A
# RESIDUAL statement -- one routed into the surrogate's own body,
# `Item::SingletonBody` -- always had the frame, from the class body it
# becomes; those are excluded from the grouping so the frame is not pushed
# twice.
#
# A `class << self` written in a METHOD body takes a different route
# entirely -- `desugar_singleton_class_defs`, a `Seq` of runtime installs on
# the receiver -- and gets the frame the same way, with the whole `Seq` as
# the group.
begin
  class Config
    class << self
      attr_accessor :setting
    end
    class << self
      self.setting = :configured
    end
  end
rescue NoMethodError => e
  puts e.backtrace.take(3)
end

begin
  class Boom
    class << self
      [1].each { |n| raise "boom #{n}" }
    end
  end
rescue RuntimeError => e
  puts e.backtrace.take(4)
end

# A body whose statements are BOTH kinds: a residual one (a CONSTANT, routed
# into the surrogate and framed by the class body it becomes) between two
# in-place ones. The run splits in three, and only ever one is on the stack.
class Mixed
  class << self
    attr_accessor :a
    LIMIT = 3
    [2].each { |n| n }
  end
end
Mixed.a = 1
p [Mixed.a, Mixed.singleton_class.const_get(:LIMIT)]

begin
  class Mixed
    class << self
      [].fetch(99)
    end
  end
rescue IndexError => e
  puts e.backtrace.take(2)
end

# The frame's LINE tracks the statement that is running, not the one the
# group started on -- and the body's locals are shared across the whole of
# it, which is why the statements could not simply be moved.
begin
  class Walked
    class << self
      x = 1
      y = x + 1
      raise "at #{y}"
    end
  end
rescue RuntimeError => e
  puts e.message
  puts e.backtrace.take(2)
end

# A `class << self` inside a METHOD body, where the frame underneath is the
# method's rather than a class body's.
class Late
  def self.install
    class << self
      raise "installing"
    end
  end
end
begin
  Late.install
rescue RuntimeError => e
  puts e.backtrace.take(2)
end

# A body in TAIL position, read for its VALUE: the frame block has to be an
# expression there rather than a statement.
tail = class Valued
  class << self
    :from_singleton
  end
end
p tail
