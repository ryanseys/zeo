# `require "date"` defines two classes the native half cannot: `Date::Error`
# (an ArgumentError subclass every invalid-date raise uses) and
# `Date::Infinity`. A feature-gated native class cannot register a
# constructible exception here, so both live in the gem's Ruby half and the
# native half raises `Date::Error` by name -- the split `StringScanner::Error`
# already takes.
require "date"

p Date::Error.superclass.to_s
p Date::Error.ancestors.include?(ArgumentError)
p Date::Error.name

# Every invalid-date raise is a `Date::Error`, and `rescue ArgumentError`
# still catches it.
begin
  Date.new(2024, 2, 30)
rescue => e
  p [e.class.to_s, e.message]
end
begin
  Date.parse("nope")
rescue ArgumentError => e
  p [e.class.to_s, e.message]
end
begin
  Date.strptime("x", "%Y")
rescue Date::Error => e
  p [e.class.to_s, e.message]
end

# `Date::Infinity` is a Numeric sentinel with a sign.
p defined?(Date::Infinity)
p Date::Infinity.superclass.to_s
p Date::Infinity.ancestors.first(4).map(&:to_s)
p Date::Infinity.instance_methods(false).sort

i = Date::Infinity.new
p [i.infinite?, i.finite?, i.zero?, i.nan?, i.to_f]
p [i > 1, i < 1, i == i, (i <=> 5), (5 <=> i)]

j = Date::Infinity.new(-1)
p [j.to_f, j.infinite?, j < 1, (j <=> i), (i <=> j)]

z = Date::Infinity.new(0)
p [z.to_f, z.nan?, z.infinite?]

p [(-i).to_f, (+i).to_f, i.abs.to_f]
p [(i <=> Float::INFINITY), (i <=> -Float::INFINITY)]
p i.coerce(3)
p (i + 1 rescue $!.class.to_s)

# A Date answers `infinite?` too.
p Date.new(2024, 1, 1).infinite?
