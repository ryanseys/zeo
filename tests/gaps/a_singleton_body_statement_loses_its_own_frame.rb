# Ruby gives a `class << self` body a backtrace frame of its own, labelled
# `singleton class`, with the enclosing `<class:Config>` frame beneath it.
#
# zeo now spells that label right wherever the frame EXISTS -- see
# a_singleton_body_frame_is_spelled_singleton_class.rb -- but a statement the
# singleton mapping handles IN PLACE has no frame at all: it is spliced into
# the enclosing class's body, which is the whole point of the retagging model,
# and so it runs in that body's frame. One frame goes missing from every
# backtrace raised through such a statement, and the innermost one is labelled
# after the class instead of after the singleton.
#
# Only a RESIDUAL statement (one routed into the surrogate's own body) gets a
# frame today. Both below are handled in place: the first reaches for `self`,
# the second names nothing at all.
#
# WHICH ARMS OF `map_class_self_items` LAND HERE (the ones a fix has to cover):
# `Item::SelfSend` -- a receiverless or explicit-`self` call, rebound onto
# `self.singleton_class` and spliced in place, which is `self.setting =` below;
# `Item::Passthrough` -- anything `mentions_self` says never consults `self`,
# which is `[1].each { ... }` below (the `raise` inside the block is a
# receiverless send, but the scan looks for `self`/ivars, not implicit-self
# sends); and `Item::RetargetSelf`. `Item::SingletonBody` is the one that
# already routes into the surrogate's own body and already gets the frame.
#
# Only the LABEL and the frame COUNT are wrong -- the line numbers zeo reports
# are the same ones CRuby reports, so the fix is a frame to push, not a
# position to correct.
#
# SHAPE OF A FIX: the mapped items of a singleton body need to run under a
# frame of their own while still being statements of the enclosing class body
# -- a frame-only HIR node that analyze's class-body walks recurse into, since
# the `def`s among them must stay visible to method registration.
#
# RULED OUT (2026-08-12): routing the executable items into the surrogate's
# own `ClassDef` body instead, which would get the frame for free. A class
# body opens its own LOCAL SCOPE, and a singleton body's locals are shared
# across the whole of it -- `class << self; x = 1; puts x; end` would break
# the moment some items moved into the surrogate body and others stayed in
# the enclosing one. The frame has to arrive without moving the statements.
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
