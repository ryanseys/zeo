# CRuby's Float rounding return type is value-based on ndigits:
# Integer when ndigits <= 0 (or absent), Float when > 0. Spinel used to
# be presence-based (any arg => Float), so round(0)/round(-1) were Float.
p 1.9.round
p 1.9.round.class
p 1.9.round(0)
p 1.9.round(0).class
p 1234.5.round(-1)
p 1234.5.round(-1).class
p 1.234.round(2)
p 1.234.round(2).class
p 1.5.ceil(0)
p 1.5.ceil(0).class
p 1.23.floor(1)
p 1.23.floor(1).class
p 19.9.truncate(-1)
p 19.9.truncate(-1).class
