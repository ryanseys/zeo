# Two pattern-matching reports.
#
# A hash pattern naming specific keys calls `#deconstruct_keys` with an
# Array of just those keys, reserving `nil` for a pattern that can take
# everything (`**rest`, `**nil`, or no keys at all). Every match passed
# `nil`, so an implementation that builds only what was asked for could not
# tell the two cases apart -- and one that BRANCHES on nil took the wrong
# branch.
#
# And `expr => pattern` has its own destructuring emitter, which binds
# direct local targets only; a nested class pattern bound nothing and
# raised nothing, so the local came out nil. Such a pattern is rewritten to
# the one-arm `case ... in ... end` it is defined to mean.

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
__END__
[:a]
matched
[:a, :c]
two
nil
3
[:b]
2
1
7
3
[1, 2]
[1, 2]
NoMatchingPatternError
