# Numbered block params (`_1.._N`) and the implicit `it` param in a
# direct instance_exec. Each numbered slot binds to the corresponding
# call-site arg, and the block body runs with self rebound to the
# receiver. The parser normalizes `it` to a single-slot numbered
# param, so it shares the same lowering.

class Box
  def initialize(v)
    @v = v
  end
end

class BoxPlus < Box
end

b = BoxPlus.new(10)

# Single numbered param.
puts b.instance_exec(7) { _1 * 2 }       #=> 14

# `it` (implicit single param).
puts b.instance_exec(8) { it + 1 }       #=> 9

# Two numbered params.
puts b.instance_exec(3, 4) { _1 + _2 }   #=> 7

# Numbered param mixed with a rebound-self ivar read.
puts b.instance_exec(5) { @v + _1 }      #=> 15

# Expression position.
n = b.instance_exec(2) { _1 * @v }
puts n                                   #=> 20

puts "done"
