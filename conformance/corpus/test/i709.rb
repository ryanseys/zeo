class Pool
  def initialize(size)
    @free = []
    i = 0
    while i < size
      @free.push(yield)
      i += 1
    end
  end
  def first
    @free[0]
  end
  def count
    @free.length
  end
end

counter = 0
p = Pool.new(2) do
  counter += 1
  "handle-#{counter}"
end
puts p.first
puts p.count

class Acc
  def initialize(n)
    @vals = []
    i = 0
    while i < n
      @vals.push(yield(i))
      i += 1
    end
  end
  def all
    @vals
  end
end

a = Acc.new(3) { |x| x * 10 }
puts a.all.inspect
