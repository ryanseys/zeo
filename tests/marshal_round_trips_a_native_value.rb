# `Marshal.load(Marshal.dump(x))` answers an equal `x` for every native value,
# and writes ruby's own bytes doing it.
#
# `Marshal` allocates before it fills, so every class that could not allocate
# could not be loaded either -- `Date`, `DateTime` and `BigDecimal` all raised.
# Two more looked WORSE than raising, because they looked like they had
# worked:
#
#   * a `Pathname` loaded back EMPTY. zeo keeps the path in its payload where
#     ruby keeps it in an `@path` ivar, and the generic object dump writes
#     ivars -- so it wrote none. `RPathname` now answers to the ruby name
#     through `ivar_pairs`/`ivar_set_named`, which fixes `instance_variables`
#     at the same time.
#   * an `Encoding` loaded back as a STRING. `Encoding._load` handed back the
#     name it was given rather than resolving it, so the round trip changed
#     the class.
#
# `Date` needed one more thing: `#marshal_load` has to fill the RECEIVER.
# Marshal registers the allocated object in its link table BEFORE calling
# `marshal_load`, so answering a fresh Date would make a second reference to
# one date load as the blank. That is why `RDate`'s six scalars moved behind
# one lock -- the only write on any path, and uncontended.

require "date"
require "set"
require "pathname"
require "bigdecimal"

SAMPLES = {
  "Date" => -> { Date.new(2026, 8, 29) },
  "DateTime" => -> { DateTime.new(2026, 8, 29, 1, 2, 3) },
  "Set" => -> { Set[1, 2, 3] },
  "Pathname" => -> { Pathname.new("/tmp/x") },
  "BigDecimal" => -> { BigDecimal("1.25") },
  "Regexp" => -> { /ab+c/i },
  "Rational" => -> { Rational(3, 4) },
  "Complex" => -> { Complex(1, 2) },
  "Range" => -> { (1..5) },
  "Symbol" => -> { :hello },
  "Encoding" => -> { Encoding::UTF_8 },
  "Exception" => -> { RuntimeError.new("boom") },
  "Errno" => -> { Errno::ENOENT.new("nope") },
}

SAMPLES.each do |name, make|
  v = make.call
  back = Marshal.load(Marshal.dump(v))
  puts "#{name}\t#{back.inspect}\t#{back == v}\t#{back.class}"
end

# The ivar the payload now answers to, which is what the dump reads.
puts "pathname ivars\t#{Pathname.new('/tmp/x').instance_variables.inspect}"

# The link table: one date referenced twice loads as ONE object, which is what
# `marshal_load` filling in place buys.
d = Date.new(2026, 8, 29)
pair = Marshal.load(Marshal.dump([d, d]))
puts "shared date\t#{pair[0]}\t#{pair[0].equal?(pair[1])}"

# `Encoding` is interned, so the round trip has to answer the SAME object, not
# an equal one.
puts "encoding identity\t#{Marshal.load(Marshal.dump(Encoding::UTF_8)).equal?(Encoding::UTF_8)}"

# The bytes, for the four whose dump shape this pass changed.
[Date.new(2026, 8, 29), Set[1], Pathname.new("/tmp/x"), Encoding::UTF_8].each do |v|
  puts "bytes #{v.class}\t#{Marshal.dump(v).inspect}"
end

# A class ruby refuses to dump is still refused: a Queue owns a condvar and a
# parked-thread count, neither of which survives a round trip.
begin
  Marshal.dump(Queue.new)
  puts "queue\tdumped"
rescue TypeError => e
  puts "queue\t#{e.class}: #{e.message}"
end
