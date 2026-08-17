# `arr.sum` on a receiver only known at run time, where a USER class owns the
# name too (`Node#sum`). The dispatch switch covers user classes only, so a
# real Array fell through to the NoMethodError default.

class Node
  def initialize(v)
    @v = v
  end

  def sum
    @v
  end
end

class Table
  attr_accessor :total, :lo, :hi
  def initialize(*args)
    if args.size == 1 && args[0].is_a?(Array)
      frequencies = args[0]
      @total = frequencies.sum
      @lo = frequencies.min
      @hi = frequencies.max
    else
      @total = 0
      @lo = 0
      @hi = 0
    end
  end
end

t = Table.new([3, 1, 2])
p t.total
p t.lo
p t.hi
p Table.new.total
p Node.new(5).sum
