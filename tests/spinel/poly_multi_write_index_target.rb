class Cell1
  attr_accessor :alive, :neighbors
  def initialize; @alive = false; @neighbors = []; end
  def link(c); @neighbors << c; end
  def cnt; @neighbors.count { |n| n.alive }; end
end
class Cell2
  attr_accessor :kind, :neighbors, :x, :y
  def initialize(x, y); @x = x; @y = y; @kind = 0; @neighbors = []; end
end
class Maze
  attr_accessor :cells
  def initialize(w, h)
    @w = w; @h = h
    @cells = Array.new(@h) { |y| Array.new(@w) { |x| Cell2.new(x, y) } }
    update_neighbors
  end
  def update_neighbors
    @cells.each_with_index do |row, y|
      row.each_with_index do |cell, x|
        if x > 0 && y > 0 && x < @w - 1 && y < @h - 1
          cell.neighbors << @cells[y - 1][x]
          cell.neighbors << @cells[y + 1][x]
          4.times do
            i = 0
            j = 1
            cell.neighbors[i], cell.neighbors[j] = cell.neighbors[j], cell.neighbors[i]
          end
        end
      end
    end
  end
end
m = Maze.new(4, 4)
p m.cells[1][1].neighbors.length
a = Cell1.new
a.link(Cell1.new)
p a.cnt
