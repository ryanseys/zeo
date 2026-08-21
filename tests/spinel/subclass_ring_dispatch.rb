# A circular doubly-linked structure whose left/right/up/down fields hold both
# a base class and its subclass -- Knuth's Dancing Links, which is where the
# report found it (#4023). Two things were wrong, and both come from the same
# place: a slot typed as a class that HAS subclasses is only its STATIC type,
# because an inherited method storing `self` types the slot as the class that
# DEFINED the method while the object can be any subclass.
#
# The read was emitted as a boxed value and assigned to a base-class local, so
# the build aborted; and where it did compile, a subclass-only method on such a
# read was refused outright:
#
#   undefined method 'size' for an instance of Node
#
# Both are answers the runtime object gives perfectly well, so the call
# dispatches at run time now.
class Node
  attr_accessor :left, :right, :up, :down, :column, :row_name

  def initialize(row_name = nil)
    @row_name = row_name
    @left = self
    @right = self
    @up = self
    @down = self
    @column = nil
  end

  def append_to_row(other)
    @left = other
    @right = other.right
    other.right.left = self
    other.right = self
    self
  end
end

class Column < Node
  attr_accessor :size, :name

  def initialize(name)
    super(nil)
    @name = name
    @size = 0
    @column = self
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
  attr_reader :solutions

  def initialize(column_names)
    @root = Column.new("__root__")
    @columns = {}
    column_names.each do |name|
      col = Column.new(name)
      col.append_to_row(@root.left)
      @columns[name] = col
    end
    @solutions = []
    @stack = []
  end

  def add_row(name, members)
    first = nil
    members.each do |member|
      col = @columns.fetch(member)
      node = Node.new(name)
      node.column = col
      node.up = col.up
      node.down = col
      col.up.down = node
      col.up = node
      col.size += 1
      first = first.nil? ? node : node.append_to_row(first.left)
    end
    self
  end

  def search
    if @root.right.equal?(@root)
      @solutions << @stack.map(&:row_name).sort
      return
    end
    col = smallest_column
    return if col.size.zero?

    col.cover
    row = col.down
    while !row.equal?(col)
      @stack.push(row)
      node = row.right
      while !node.equal?(row)
        node.column.cover
        node = node.right
      end
      search
      @stack.pop
      node = row.left
      while !node.equal?(row)
        node.column.uncover
        node = node.left
      end
      row = row.down
    end
    col.uncover
  end

  def remaining_columns
    names = []
    col = @root.right
    while !col.equal?(@root)
      names << "#{col.name}(#{col.size})"
      col = col.right
    end
    names
  end

  private

  def smallest_column
    best = @root.right
    col = best.right
    while !col.equal?(@root)
      best = col if col.size < best.size
      col = col.right
    end
    best
  end
end
k = ExactCover.new(%w[1 2 3])
k.add_row("A", %w[1 2])
k.add_row("B", %w[3])
k.search
p k.solutions
