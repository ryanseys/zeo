require "set"

s = Set.new([1, 2])
t = Set.new(s.map { |x| x + 1 })
p t.to_a

u = Set.new([[1, 2]])
p Set.new(u.map { |x| x }).size
p Set.new(u.to_a.map { |r, c| [r, c] }).size

m = s.map { |x| x * 10 }
p Set.new(m).to_a

class Box
  def initialize(enum = nil)
    @data = []
    if enum
      src = enum.to_a
      if block_given?
        src.each { |x| @data << yield(x) }
      else
        src.each { |x| @data << x }
      end
    end
  end
  def items = @data
  def each_mapped
    r = []
    i = 0
    while i < @data.length
      r << yield(@data[i])
      i += 1
    end
    r
  end
end

b = Box.new([1, 2])
p Box.new(b.each_mapped { |x| x + 1 }).items
p Box.new(b.each_mapped { |x| x * 3 }) { |y| y + 100 }.items
