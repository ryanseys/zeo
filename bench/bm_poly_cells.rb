# Polyvariance-rich exerciser. A `Cell.set(v)` accepts mixed-type
# `v` across call sites (string / int / array), so `@value` is a
# genuinely poly ivar and `set`'s param is a genuinely poly param.
# `render_one(v)` dispatches by is_a? — another poly param whose
# return narrows by branch but spinel currently keeps the
# function-level return as poly. Shape mirrors real-blog's
# Active Record setter pattern in ~80 lines, so SP_POLY_REPORT
# counts move visibly with each backward-inference refinement.

class Cell
  attr_accessor :value

  def initialize
    @value = ""
  end

  def set(v)
    @value = v
  end

  def get
    @value
  end
end

class Sheet
  def initialize
    @cells = []
    @count = 0
  end

  def put(v)
    c = Cell.new
    c.set(v)
    @cells.push(c)
    @count = @count + 1
  end

  def count
    @count
  end

  def cells
    @cells
  end
end

def render_one(v)
  if v.is_a?(String)
    return v
  end
  if v.is_a?(Integer)
    return v.to_s
  end
  if v.is_a?(Array)
    return "[" + v.length.to_s + " items]"
  end
  "?"
end

s = Sheet.new
s.put("hello")
s.put(42)
s.put([1, 2, 3])
s.put("world")
s.put(99)

puts "count: " + s.count.to_s

arr = s.cells
i = 0
while i < arr.length
  puts render_one(arr[i].get)
  i = i + 1
end
