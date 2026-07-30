# A `return` inside a block exits the method the BLOCK was written in, however
# many frames it has to unwind through. Every method it passes on the way is
# just a relay: one that catches a `Signal::Return` of its own -- because it has
# a `begin` or an escaping block -- must still let this one through.
def one
  [1, 2, 3].each { |x| yield x }
  :fell
end
def c1; one { |x| return x }; :no; end
p c1

# Through two nested blocks and an explicit `&b`.
def two(&b)
  [1, 2].each { |x| [3, 4].each { |y| b.call(x * y) } }
  :fell
end
def c2; two { |v| return v }; :no; end
p c2

# Through an Enumerator, and through a lazy chain.
def three
  Enumerator.new { |y| y << 1; y << 2 }.each { |v| yield v }
  :fell
end
def c3; three { |v| return v }; :no; end
p c3

def lazily
  [1, 2, 3].lazy.map { |x| x * 2 }.each { |v| yield v }
  :fell
end
def c4; lazily { |v| return v }; :no; end
p c4

# Through a relay that has a `begin`/`ensure` -- no `return` of its own, so it
# has nothing to catch.
def ensured
  begin
    yield 9
  ensure
    nil
  end
  :fell
end
def c5; ensured { |v| return v }; :no; end
p c5

def rescued
  yield 8
rescue StandardError
  :r
else
  :fell
end
def c6; rescued { |v| return v }; :no; end
p c6

# Through a relay that has a `return` of its own INSIDE the begin: it really
# does catch, and must still tell the two apart.
def both
  begin
    yield 7
    return :inner
  rescue StandardError
    :r
  end
end
def c7; both { |v| return v }; :no; end
p c7
p both { :ignored }

# Through several frames, and through `__send__`.
def deep3(&b) = deep2(&b)
def deep2(&b) = deep1(&b)
def deep1(&b)
  [1, 2].each { |x| b.call(x) }
  :fell
end
def c8; deep3 { |v| return v }; :no; end
p c8

class Relay
  def go(&b)
    begin
      __send__(:inner, &b)
    rescue StandardError
      :r
    end
    :fell
  end
  def inner = yield(:through_send)
end
def c9; Relay.new.go { |v| return v }; :no; end
p c9

# A `return` whose home has already unwound is a LocalJumpError, not a jump
# into a dead frame.
def escapee = proc { return :gone }
begin
  escapee.call
rescue LocalJumpError => e
  p [:local_jump, e.reason]
end

# A lambda's `return` is its own, and never leaves the caller.
def lam
  l = lambda { return :from_lambda }
  [l.call, :after]
end
p lam
