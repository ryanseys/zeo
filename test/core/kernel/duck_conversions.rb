# Ruby's implicit-conversion protocols: `&obj` converts through `to_proc`,
# `**obj` through `to_hash`, and a splice/auto-splat value through `to_ary`.
# Each is duck-typed -- any object answering the method participates.

# --- &obj -> to_proc -------------------------------------------------------

# A Symbol is the familiar case.
p [1, 2, 3].map(&:to_s)
p %w[a b].map(&:upcase)

# A Proc passes straight through.
doubler = ->(x) { x * 2 }
p [1, 2, 3].map(&doubler)

# A Method object converts via Method#to_proc.
def triple(x) = x * 3
p [1, 2, 3].map(&method(:triple))

# Any object defining to_proc converts.
class Dbl
  def to_proc
    ->(x) { x * 2 }
  end
end
p [1, 2, 3].map(&Dbl.new)
d = Dbl.new
p [10, 20].map(&d)

# nil means "no block at all".
def block_given_here(&blk)
  blk.nil?
end
p block_given_here(&nil)

# An object whose to_proc answers a non-Proc is a TypeError.
class BadProc
  def to_proc = :not_a_proc
end
begin
  [1].map(&BadProc.new)
rescue TypeError => e
  puts "TypeError: #{e.message}"
end

# An object with no to_proc at all is a TypeError.
class NoProc; end
begin
  [1].map(&NoProc.new)
rescue TypeError => e
  puts "TypeError: #{e.message}"
end

# --- **obj -> to_hash ------------------------------------------------------

class Opts
  def to_hash = { a: 1, b: 2 }
end

def take(a:, b:) = [a, b]
p take(**Opts.new)

# Into a keyword-rest callee.
def collect(**o) = o
p collect(**Opts.new)

# An explicit keyword alongside the converted splat.
p collect(x: 9, **Opts.new)

# A later key wins, matching Hash#merge order.
p collect(a: 99, **Opts.new)

# A converter held in a local.
o = Opts.new
p take(**o)

# A plain Hash splats without any conversion.
h = { a: 5, b: 6 }
p take(**h)
p collect(c: 7, **h)

# An object with no to_hash is a TypeError.
class NoHash; end
begin
  collect(**NoHash.new)
rescue TypeError => e
  puts "TypeError: #{e.message}"
end

# --- to_ary ----------------------------------------------------------------

class Pair
  def initialize(a, b)
    @a = a
    @b = b
  end
  def to_ary = [@a, @b]
end

# Block auto-splat coerces through to_ary.
def one(v)
  yield v
end
p(one(Pair.new(1, 2)) { |a, b| [a, b] })

# Multiple assignment destructures through to_ary.
x, y = Pair.new(3, 4)
p [x, y]

# An Array splice coerces the right-hand side through to_ary.
arr = [1, 2, 3]
arr[1, 1] = Pair.new(7, 8)
p arr

# A `for` loop's destructuring target.
[[1, 2]].each do |a, b|
  p [a, b]
end
__END__
["1", "2", "3"]
["A", "B"]
[2, 4, 6]
[3, 6, 9]
[2, 4, 6]
[20, 40]
true
TypeError: can't convert BadProc to Proc (BadProc#to_proc gives Symbol)
TypeError: no implicit conversion of NoProc into Proc
[1, 2]
{a: 1, b: 2}
{x: 9, a: 1, b: 2}
{a: 1, b: 2}
[1, 2]
[5, 6]
{c: 7, a: 5, b: 6}
TypeError: no implicit conversion of NoHash into Hash
[1, 2]
[3, 4]
[1, 7, 8, 3]
[1, 2]
