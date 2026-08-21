# GAP -- imported from the spinel corpus at fa06b601.
#
# A hash pattern naming specific keys calls `#deconstruct_keys` with `nil`
# where CRuby passes an ARRAY of the keys the pattern names.
#
#   case obj; in {a:}     CRuby calls deconstruct_keys([:a])
#                         zeo   calls deconstruct_keys(nil)
#
# `nil` is what CRuby passes for a pattern with a `**rest`, and it means
# "build the whole hash". So an object whose `deconstruct_keys` optimises for
# the named keys -- the reason the argument exists -- does more work in zeo,
# and one that BRANCHES on `nil` takes the wrong branch.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# Two pattern-matching reports.
#
# A hash pattern naming specific keys calls #deconstruct_keys with an Array of
# just those keys, reserving nil for a pattern that can take everything
# (`**rest`). Every match passed nil, so an implementation that builds only
# what was asked for could not tell the two cases apart (#4046).
#
# And `expr => pattern` has its own destructuring emitter, which binds direct
# local targets only. A nested class pattern bound nothing at all and raised
# nothing either, so the local came out nil (#4047). Such a pattern is rewritten
# to the one-arm `case ... in ... end` it is defined to mean.
class Probe
  def deconstruct_keys(keys)
    p keys
    { a: 1, b: 2, c: 3 }
  end
end

case Probe.new
in { a: } then puts "matched"
end

case Probe.new
in { a:, c: } then puts "two"
end

case Probe.new
in { **rest } then p rest.size
end

Probe.new => { b: bv }
p bv

class Node
  attr_reader :left
  def initialize(left) = @left = left
  def deconstruct_keys(keys) = { left: @left }
end

class Lit
  attr_reader :value
  def initialize(value) = @value = value
  def deconstruct_keys(keys) = { value: @value }
end

Node.new(Lit.new(1)) => { left: Lit(value: lv) }
p lv

Node.new(Lit.new(7)) => { left: { value: v2 } }
p v2

case Node.new(Lit.new(3))
in { left: Lit(value: cv) } then p cv
end

# the flat shapes the dedicated emitter still handles
{ a: 1, b: 2 } => { a:, b: }
p [a, b]
[1, 2] => [x, y]
p [x, y]

begin
  Node.new(Lit.new(1)) => { left: Integer => bad }
rescue NoMatchingPatternError, NoMatchingPatternKeyError => e
  p e.class
end
