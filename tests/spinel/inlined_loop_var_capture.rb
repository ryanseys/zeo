# A proc lifted out of a NESTED block reads the enclosing INLINED loop's
# variable. The outer `each_with_index` is spliced into a C loop, so `y` binds
# to a plain C slot; the inner block is materialized as a proc by the runtime
# dispatch and needs a cell to read `y` through. The cell now shadows the slot:
# the loop binds the slot as before and the body's opening line copies it in.

class Cell
  def initialize
    @n = []
  end
  def add_neighbor(c)
    @n << c
  end
  def count
    @n.length
  end
end

class Ring
  def initialize(v)
    @v = v
  end
  def each(&blk)
    blk.call(@v)
  end
  def each_with_index(&blk)
    blk.call(@v, 0)
  end
end

def pick(f, rows)
  f ? rows : Ring.new(1)
end

class Grid
  def initialize(w, h)
    @width = w
    @height = h
    @cells = Array.new(@height) { Array.new(@width) { Cell.new } }
    link_neighbors
  end

  def link_neighbors
    @cells.each_with_index do |column, y|
      pick(true, column).each_with_index do |cell, x|
        (-1..1).each do |dy|
          (-1..1).each do |dx|
            next if dx == 0 && dy == 0
            ny = (y + dy + @height) % @height
            nx = (x + dx + @width) % @width
            cell.add_neighbor(@cells[ny][nx])
          end
        end
      end
    end
  end

  def total
    @cells.map { |r| r.map { |c| c.count }.sum }.sum
  end
end

p Grid.new(4, 3).total
