# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# `c.neighbours` is filled with the cells including `c` itself.
#@ gccheck: cycle leak: 6 objects (Array x3, Cell x3)
# An ivar seeded empty in initialize (`@n = []` -> int-array) must widen when an
# external attr-writer assigns a wider value through a *poly* receiver -- e.g. an
# element read from a poly array/hash (#1508). Inference attributes the write to
# the unique class defining that attr-writer.
class Cell
  attr_accessor :alive, :neighbours
  def initialize(a)
    @alive = a
    @neighbours = []
  end
  def alive_count = neighbours.count(&:alive)
end
class World
  def initialize
    @cells = { "a" => Cell.new(true), "b" => Cell.new(false), "c" => Cell.new(true) }
  end
  def at(k) = @cells[k]                      # poly value from a poly hash
  def link
    c = @cells["a"]                          # c is poly
    c.neighbours = ["b", "c", "a"].filter_map { |k| at(k) }
    c.alive_count
  end
end
p World.new.link                             # b=false, c=true, a=true -> 2
__END__
2
