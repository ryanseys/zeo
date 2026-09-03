# A non-lambda block yielded EXACTLY ONE Array argument spreads it across its
# positional parameters -- the rule that makes `Hash#each { |k, v| }` and
# `[[1, 2]].each { |a, b| }` read naturally. Whether a given block splats at
# all is decided by its own parameter shape (CRuby's `has_lead &&
# !ambiguous_param0`), so every shape below is covered.

def one(v)
  yield v
end

# No leading required parameter -> never splats.
p(one([1, 2]) { |*a| a })
p(one([1, 2]) { |*a, **k| [a, k] })

# Exactly ONE positional slot -> the one-parameter case is exempt.
p(one([1, 2]) { |a| a })
p(one([1, 2]) { |a, **k| [a, k] })

# A lead parameter plus any second positional slot -> splats.
p(one([1, 2]) { |a, b| [a, b] })
p(one([1, 2]) { |a, b, **k| [a, b, k] })
p(one([1, 2]) { |a, *b| [a, b] })
p(one([1, 2]) { |a, b = 5| [a, b] })
p(one([1, 2, 3]) { |a, *b, c| [a, b, c] })

# More parameters than elements nil-fills; fewer drops the extras.
p(one([1]) { |a, b| [a, b] })
p(one([1, 2, 3]) { |a, b| [a, b] })

# Two yielded values are already separate arguments -- no splat involved.
def two(a, b)
  yield a, b
end
p(two(1, 2) { |a, b| [a, b] })
p(two([1, 2], 3) { |a, b| [a, b] })

# The idioms this exists for.
[[1, 2], [3, 4]].each { |a, b| p [a, b] }
p({ a: 1, b: 2 }.map { |k, v| "#{k}=#{v}" })
p [[1, 2], [3, 4]].map { |a, b| a + b }
p [[1, 2], [3, 4]].to_h { |a, b| [b, a] }

# A single-element array still splats (the element binds, the rest nil-fill).
p(one([9]) { |a, b| [a, b] })

# A lambda is STRICT: no auto-splat, and a wrong count raises.
strict = ->(a, b) { [a, b] }
p strict.call(1, 2)
begin
  strict.call([1, 2])
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end

# The trailing-Hash-to-keywords split happens BEFORE the splat, so a Hash
# element lands in a positional slot rather than the keyword rest.
p(one([1, { x: 9 }]) { |a, b, **k| [a, b, k] })

# Keywords yielded alongside a value are not part of the splat decision.
def with_kw(v)
  yield v, x: 9, y: 8
end
p(with_kw(1) { |v, x:, **k| [v, x, k] })

# A user object defining to_ary coerces through it, like CRuby's
# rb_check_array_type.
class Pair
  def initialize(a, b)
    @a = a
    @b = b
  end

  def to_ary
    [@a, @b]
  end
end
p(one(Pair.new(1, 2)) { |a, b| [a, b] })
p(one(Pair.new(1, 2)) { |a| a.class })

# An object WITHOUT to_ary binds as a single argument.
class Opaque; end
p(one(Opaque.new) { |a, b| [a.class, b] })
__END__
[[1, 2]]
[[[1, 2]], {}]
[1, 2]
[[1, 2], {}]
[1, 2]
[1, 2, {}]
[1, [2]]
[1, 2]
[1, [2], 3]
[1, nil]
[1, 2]
[1, 2]
[[1, 2], 3]
[1, 2]
[3, 4]
["a=1", "b=2"]
[3, 7]
{2 => 1, 4 => 3}
[9, nil]
[1, 2]
ArgumentError: wrong number of arguments (given 1, expected 2)
[1, {x: 9}, {}]
[1, 9, {y: 8}]
[1, 2]
Pair
[Opaque, nil]
