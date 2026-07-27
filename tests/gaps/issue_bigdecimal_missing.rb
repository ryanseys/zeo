# bigdecimal is a CRuby C extension (arbitrary-precision decimal arithmetic)
# that zeo has no native implementation of -- `require "bigdecimal"` raises
# LoadError.
require "bigdecimal"
p BigDecimal("1.5")
