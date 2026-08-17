# GAP -- imported from the spinel corpus at c55d9bdb.
# Hash#compare_by_identity / frozen-hash behaviour diverges: a Symbol key
# reads back as its value.
#
# Five surfaces from the conformance wave, each in a different corner.

# Hash#all?/any?/none?/one? with a CLASS pattern is a kind-of test over the
# [key, value] pairs; comparing the pair to the class value by equality
# answered false for every pair
h = { a: 1, b: 2 }
p h.all?(Array)
p h.any?(Array)
p h.none?(Array)
p h.one?(Array)
p h.all?(String)
p h.any?([:a, 1])

# fetch: a block supersedes a positional default, as CRuby's warning says
p({ a: 1 }.fetch(:z, 9) { |k| k })
p({ a: 1 }.fetch(:z, 9))
p({ a: 1 }.fetch(:z) { |k| k })
p({ a: 1 }.fetch(:a, 9))

# an op-assign is a write, so a frozen receiver refuses it
class Counter
  def initialize; @n = 1; end
  def n; @n; end
  def bump; @n += 1; end
  def set(v); @n = v; end
end

c = Counter.new.freeze
r = (c.bump rescue $!.class); p r
p c.n
r = (c.set(5) rescue $!.class); p r
p c.n

live = Counter.new
live.bump
p live.n

# printf goes through the Ruby formatter, so a Symbol or nil reaches %s as a
# value rather than a raw slot, and %p / %<name> / %{name} mean what they mean
printf("%s\n", :sym)
printf("%s|\n", nil)
printf("%p\n", nil)
printf("%<n>04d\n", n: 7)
printf("%{k}\n", k: 3)
printf("%05.2f %x %s\n", 1.5, 255, "s")

# Kernel#Hash takes an empty Hash literal
r = (Hash({}) rescue $!.class); p r
p Hash({ a: 1 })
p Hash([])
p Hash(nil)
