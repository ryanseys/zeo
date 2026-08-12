# A module mixed into a subclass of a VALUE builtin, whose method calls
# `super`, crashes the generated program:
#
#   thread 'ruby-main' panicked at crates/zeo-rt/src/builtins/array.rs:144:
#   internal error: entered unreachable code: Array table row dispatched on a
#   non-Array receiver
#
# The same `super` written directly in the subclass works (the second pair
# below), so this is about reaching the builtin THROUGH a module in the
# ancestry rather than about the value-super channel itself.
#
# Not a sharing bug: `ZEO_SHARE=0` panics identically. Found while building
# divergence shapes for `codegen::class_query`, whose `ValueSuper` question
# exists precisely because this channel is per-receiver -- `DoubledArray`
# reaches `Array#size` where `DoubledPlain` reaches a user body.

module Doubled
  def size = super * 2
end

class DoubledArray < Array
  include Doubled
end

class Base
  def size = 5
end

class DoubledPlain < Base
  include Doubled
end

p DoubledArray.new(3).size
p DoubledPlain.new.size

# Written directly in the subclass, with no module in between, the same `super`
# is fine today.
class DirectArray < Array
  def size = super * 2
end

p DirectArray.new(3).size
