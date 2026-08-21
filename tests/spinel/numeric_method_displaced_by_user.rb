# A class defining `abs` (or `round`, `succ`) takes over that name's poly
# dispatch. An Integer arriving there had no arm, so a boxed ivar holding one
# raised "undefined method 'abs' for an instance of Integer" -- and where the
# result was consumed as the user method's type, the builtin arm's boxed answer
# was read as that object and the program crashed (#4012).
class Fixed
  attr_reader :units

  def initialize(units) = @units = units
  def abs = Fixed.new(@units.abs)
  def round = Fixed.new(@units.round)
  def succ = Fixed.new(@units.succ)
  def coerce(other) = [Fixed.new(other), self]

  def probe
    p @units.abs
    p @units.round
    p @units.succ
    p @units.ceil
    p @units.floor
    p @units.truncate
  end
end

Fixed.new(-1999).probe
p Fixed.new(-7).abs.units
p Fixed.new(3).succ.units

# the user method still wins for its own instances
f = [Fixed.new(-2)][0]
p f.abs.units
