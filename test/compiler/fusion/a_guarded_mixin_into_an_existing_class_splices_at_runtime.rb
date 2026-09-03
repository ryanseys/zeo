# A mixin into an EXISTING class under a runtime-undecidable guard is a
# runtime ancestry edit: the directive runs as the send it is, exactly when
# the guard passes (hammertime's `unless ::Object < Hammertime` idempotence
# guard, clusterer's `$LINALG == true`). Call sites the module could newly
# answer or override are de-optimized, so a folded call cannot bypass the
# splice -- and before the guard runs, the ancestry is untouched.
module Warmth
  def warmed? = true
end

class Thermos
  def temperature = 20
end

$want_warmth = true
p Thermos.ancestors.include?(Warmth)
if $want_warmth
  class Thermos
    include Warmth
  end
end
p Thermos.ancestors.include?(Warmth)
p Thermos < Warmth
p Thermos.new.warmed?

# A prepend that OVERRIDES a method the class already answers -- the
# de-optimization is what keeps a statically-resolved `capacity` from
# reaching the class's own body underneath the override.
module Doubled
  def capacity = super * 2
end

class Bottle
  def capacity = 350
end

unless Bottle < Doubled
  class Bottle
    prepend Doubled
  end
end
p Bottle.new.capacity
p Bottle.ancestors.first == Doubled

# A FALSE guard splices nothing.
module Chill
  def frosty? = true
end

$never = false
if $never
  class Thermos
    include Chill
  end
end
p Thermos.ancestors.include?(Chill)
begin
  Thermos.new.frosty?
rescue NoMethodError => e
  puts e.class
end

# `extend` under the guard: the module's methods arrive as class methods.
module Stockroom
  def stocked? = true
end

class Pantry; end

$inventory = 1
if $inventory == 1
  class Pantry
    extend Stockroom
  end
end
p Pantry.stocked?

# The singleton half (`class << self; prepend M`), overriding an existing
# class method.
module Announcing
  def open = "announcing: " + super
end

class Shop
  def self.open = "9am"
end

$noisy = true
if $noisy
  class Shop
    class << self
      prepend Announcing
    end
  end
end
p Shop.open

# The module's hook fires when (and only when) the guard passes, with the
# edited class as its argument.
module Tracked
  def self.included(base)
    $included_into = base.name
  end
end

class Ledger; end

$track = true
if $track
  class Ledger
    include Tracked
  end
end
p $included_into

# hammertime's exact shape: Object itself, behind its own idempotence probe.
module Hammer
  def hammer_time? = true
end

unless ::Object < Hammer
  class ::Object
    include ::Hammer
  end
end
p 42.hammer_time?
p "anything".hammer_time?
puts "still running"
__END__
false
true
true
true
700
true
false
NoMethodError
true
"announcing: 9am"
"Ledger"
true
true
still running
