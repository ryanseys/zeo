class Cell
  attr_reader :n
  def initialize(n); @n = n; end
end

def build
  path = []
  path << [Cell.new(1), -1]
  path << [Cell.new(2), 0]
  path
end

path = build
i = 0
while i < 2
  cell, prev = path[i]
  p [cell.n, prev]
  i += 1
end
