# `.new` was the last call site still binding arguments by hand, and it
# only handled required + optional -- `def initialize(*values)` was a
# flat compile-time rejection.

class Splat
  def initialize(*v); @v = v; end
  attr_reader :v
end
p Splat.new.v
p Splat.new(1).v
p Splat.new(1, 2, 3).v

class Mixed
  def initialize(a, b = 5, *rest, last)
    @all = [a, b, rest, last]
  end
  attr_reader :all
end
p Mixed.new(1, 9).all
p Mixed.new(1, 2, 3, 4, 9).all

class Kw
  def initialize(a, k:, j: 7, **rest)
    @all = [a, k, j, rest]
  end
  attr_reader :all
end
p Kw.new(1, k: 2).all
p Kw.new(1, k: 2, j: 3, z: 4).all

class Post
  def initialize(a, *m, y, z); @all = [a, m, y, z]; end
  attr_reader :all
end
p Post.new(1, 2, 3, 4, 5).all
p Post.new(1, 2, 3).all
__END__
[]
[1]
[1, 2, 3]
[1, 5, [], 9]
[1, 2, [3, 4], 9]
[1, 2, 7, {}]
[1, 2, 3, {z: 4}]
[1, [2, 3], 4, 5]
[1, [], 2, 3]
