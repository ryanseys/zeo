# A block passed to a poly receiver, capturing an enclosing local.
#
# The dispatch materializes the block as a real proc once, ahead of the switch,
# for any candidate that takes a real &block or is served by a proc-form clone.
# That proc is as escaping as any other, so its captures need heap cells -- but
# nothing decided that, because no single resolved method names the target the
# way a monomorphic receiver does. Codegen then met a capture with no storage
# and refused the whole call.
#
# Reachable only once a yielding method became poly-dispatchable at all: before
# that the arm did not exist, so the same source raised NoMethodError at run
# time instead.

class A
  def each_x
    yield 1
    yield 2
  end
end

class B
  def each_x
    yield 10
  end
end

def pick(n)
  n == 1 ? A.new : B.new
end

# read of a captured hash
h = { 1 => "one", 2 => "two", 10 => "ten" }
out = []
pick(1).each_x { |k| out << h[k] }
pick(2).each_x { |k| out << h[k] }
p out

# write to a captured integer: the cell is what makes the write visible here
total = 0
pick(1).each_x { |k| total += k }
p total
pick(2).each_x { |k| total += k }
p total

# a captured string, reassigned in the block
label = ""
pick(1).each_x { |k| label = label + k.to_s }
p label

# the same, through a method taking a real &block rather than yielding
class P
  def run(&blk)
    blk.call(3)
  end
end

class Q
  def run(&blk)
    blk.call(4)
  end
end

def pick2(n)
  n == 1 ? P.new : Q.new
end

base = 100
p pick2(1).run { |v| base + v }
p pick2(2).run { |v| base + v }

acc = []
pick2(1).run { |v| acc << v }
pick2(2).run { |v| acc << v }
p acc

# a capture read only inside a block nested in the passed block
mult = 3
sums = []
pick(1).each_x { |k| [k].each { |e| sums << e * mult } }
p sums
