# clone/dup dispatch the user's initialize_copy (deep-copying a
# shared member), and its bare `super` resolves Object's default no-op
# hook instead of panicking.

class Board
  def initialize
    @table = [[1], [2]]
  end
  def initialize_copy(orig)
    super
    @table = @table.clone
  end
  def push_row
    @table.push([9])
  end
  def size
    @table.length
  end
end
b = Board.new
c = b.clone
c.push_row
puts c.size
puts b.size
__END__
3
2
