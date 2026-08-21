# A Hash constant passed to a parameter that defaults to `{}`. Two faults, both
# silent: the loop ran zero times, and then its writes went nowhere (#4041).
#
# The poly `each` asked sp_poly_arr_len_ex for the receiver's length, and that
# knew only the three poly-VALUED hash variants -- a String-keyed one answered
# 0 and the block simply never ran.
#
# With it running, the `@units = {}` literal was symbol-keyed while the keys
# written into it were Strings, so every entry was dropped. The key sites can
# disagree across fixpoint rounds (a block parameter of a poly iteration reads
# as poly on one round and as the default literal's own kind on another); the
# poly-keyed hash is the only representation that holds both, so it now wins
# over a narrower mark rather than losing to whichever landed first.
class Quantity
  attr_reader :units
  def initialize(units = {})
    @units = {}
    units.each { |u, e| @units[u] = e }
  end
end
KILO = { 'kg' => 1 }
p Quantity.new(KILO).units
p Quantity.new({ 'g' => 2, 'mg' => 3 }).units
p Quantity.new.units

SYMS = { a: 1 }
p Quantity.new(SYMS).units

INTS = { 1 => 'one' }
p Quantity.new(INTS).units

# the plain function shape, and the iteration count itself
def count(src = {})
  n = 0
  src.each { |k, v| n += 1 }
  n
end
p count(KILO)
p count(SYMS)
p count(INTS)
p count({ 'x' => 1, 'y' => 2 })
p count

# every hash kind through a poly each
def keys_of(src = {})
  out = []
  src.each { |k, v| out << k }
  out
end
p keys_of({ 'a' => 'b' })
p keys_of({ 1 => 2 })
p keys_of({ 1 => 'x' })
p keys_of({ a: [1] })
