# `define_method` HARDENS proc semantics: the installed method arity-checks
# like a `def`, and an undeclared keyword is an error. The block binder
# underneath is lenient on both counts -- it pads a missing positional with
# nil and drops an unknown keyword -- so the shape is checked before the
# body runs.
#
# Three rules make this narrower than it sounds, and each is a row below.
#
#   * A trailing kw-marked Hash is only KEYWORDS when the callee declares
#     some. `def f(*a); f(k: 1)` puts the hash in `a`, which is also exactly
#     how a `ruby2_keywords` splat forwards one.
#   * Only a MARKED hash counts even then. A genuine positional Hash must
#     not be peeled, or the arity reported for it is wrong.
#   * A body with no Ruby SOURCE was built by the runtime -- an accessor
#     from `attr_accessor`, an Enumerator shuttle, `Symbol#to_proc` -- and
#     has no Ruby-level shape to check against.

def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end

# --- Positional arity -----------------------------------------------------
c = Class.new
c.send(:define_method, :two) { |a, b| [a, b] }
show { c.new.two(1) }
show { c.new.two(1, 2) }
show { c.new.two(1, 2, 3) }

c.send(:define_method, :opt) { |a, b = :d| [a, b] }
show { c.new.opt(1) }
show { c.new.opt(1, 2) }
show { c.new.opt }

c.send(:define_method, :splat) { |a, *rest| [a, rest] }
show { c.new.splat(1) }
show { c.new.splat(1, 2, 3) }
show { c.new.splat }

# A lambda source was already strict, and stays so.
c.send(:define_method, :lam, ->(a, b) { [a, b] })
show { c.new.lam(1) }

# --- Keywords -------------------------------------------------------------
k = Class.new { def kw(a:) = a }
show { k.new.kw(a: 1, b: 2) }
show { k.new.method(:kw).call(a: 1, z: 9) }
show { k.new.send(:kw, a: 1, q: 3) }
show { k.new.kw(a: 1) }
show { k.new.kw }

kr = Class.new { def kw(a:, **rest) = [a, rest] }
show { kr.new.kw(a: 1, b: 2) }

# A method with NO declared keywords takes the hash positionally.
s = Class.new { def any(*a) = a }
show { s.new.any(k: 1) }
show { s.new.any(1, k: 2) }

one = Class.new { def one(x) = x }
show { one.new.one(k: 1) }

# An UNMARKED trailing Hash is a positional, and must not be peeled.
kh = Class.new { def kw(a:) = a }
show { kh.new.kw({ a: 1 }) }

# --- Runtime-built bodies keep working ------------------------------------
o = Object.new
o.singleton_class.attr_accessor :tag
o.tag = "x"
show { o.tag }
show { [1, 2].map(&:to_s) }
show { [[1, 2]].each_with_object([]) { |(a, b), acc| acc << a + b } }
__END__
ArgumentError: wrong number of arguments (given 1, expected 2)
[1, 2]
ArgumentError: wrong number of arguments (given 3, expected 2)
[1, :d]
[1, 2]
ArgumentError: wrong number of arguments (given 0, expected 1..2)
[1, []]
[1, [2, 3]]
ArgumentError: wrong number of arguments (given 0, expected 1+)
ArgumentError: wrong number of arguments (given 1, expected 2)
ArgumentError: unknown keyword: :b
ArgumentError: unknown keyword: :z
ArgumentError: unknown keyword: :q
1
ArgumentError: missing keyword: :a
[1, {b: 2}]
[{k: 1}]
[1, {k: 2}]
{k: 1}
ArgumentError: wrong number of arguments (given 1, expected 0; required keyword: a)
"x"
["1", "2"]
[3]
