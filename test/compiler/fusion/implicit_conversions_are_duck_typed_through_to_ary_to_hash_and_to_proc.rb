class Pair
  def initialize(a, b)
    @a = a
    @b = b
  end
  def to_ary = [@a, @b]
end
class Opts
  def to_hash = { a: 1, b: 2 }
end
class Dbl
  def to_proc = ->(x) { x * 2 }
end

# to_ary: block auto-splat, multi-assign, and a splice RHS.
def one(v)
  yield v
end
p(one(Pair.new(1, 2)) { |a, b| [a, b] })
x, y = Pair.new(3, 4)
p [x, y]
arr = [1, 2, 3]
arr[1, 1] = Pair.new(7, 8)
p arr

# to_hash: a `**` splat.
def take(a:, b:) = [a, b]
p take(**Opts.new)

# to_proc: an `&` block argument.
p [1, 2].map(&Dbl.new)
__END__
[1, 2]
[3, 4]
[1, 7, 8, 3]
[1, 2]
[2, 4]
