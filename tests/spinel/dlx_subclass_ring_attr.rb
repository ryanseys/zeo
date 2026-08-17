# A ring whose fields hold both a base class and its subclass: reading one
# yields a polymorphic value, and the local it is bound to has to be poly too.
# The callee's return widened only after the local was derived, so the C
# assigned an sp_RbVal to an sp_Node * and the build failed (#3964).
class Node
  attr_accessor :left, :right, :up, :down, :column

  def initialize
    @left = self
    @right = self
    @up = self
    @down = self
    @column = nil
  end

  def link_right(node)
    node.right = @right
    node.left = self
    @right.left = node
    @right = node
    node
  end

  def link_down(node)
    node.down = @down
    node.up = self
    @down.up = node
    @down = node
    node
  end
end

class Column < Node
  attr_accessor :size, :name

  def initialize(name)
    super()
    @size = 0
    @name = name
  end

  def cover
    right.left = left
    left.right = right
    i = down
    while !i.equal?(self)
      j = i.right
      while !j.equal?(i)
        j.down.up = j.up
        j.up.down = j.down
        j.column.size -= 1
        j = j.right
      end
      i = i.down
    end
  end

  def uncover
    i = up
    while !i.equal?(self)
      j = i.left
      while !j.equal?(i)
        j.column.size += 1
        j.down.up = j
        j.up.down = j
        j = j.left
      end
      i = i.up
    end
    right.left = self
    left.right = self
  end
end

class ExactCover
  def initialize(columns, rows)
    @root = Column.new("root")
    @cols = {}
    columns.each do |cname|
      col = Column.new(cname)
      @root.link_right(col)
      @cols[cname] = col
    end
    rows.each do |row|
      first = nil
      row.each do |cname|
        col = @cols[cname]
        node = Node.new
        node.column = col
        col.link_down(node)
        col.size += 1
        first = first.nil? ? node : first.link_right(node)
      end
    end
    @solution = []
    @results = []
  end

  def smallest_column
    best = nil
    c = @root.right
    while !c.equal?(@root)
      best = c if best.nil? || c.size < best.size
      c = c.right
    end
    best
  end

  def remaining_columns
    n = 0
    c = @root.right
    while !c.equal?(@root)
      n += 1
      c = c.right
    end
    n
  end

  def search
    if @root.right.equal?(@root)
      @results << @solution.map { |n| n.column.name }.sort
      return
    end
    col = smallest_column
    return if col.nil? || col.size.zero?
    col.cover
    r = col.down
    while !r.equal?(col)
      @solution.push(r)
      j = r.right
      while !j.equal?(r)
        j.column.cover
        j = j.right
      end
      search
      @solution.pop
      j = r.left
      while !j.equal?(r)
        j.column.uncover
        j = j.left
      end
      r = r.down
    end
    col.uncover
  end

  def solve
    search
    @results
  end
end

ec = ExactCover.new(%w[1 2 3], [%w[1 2], %w[3], %w[1], %w[2 3]])
p ec.solve.sort
