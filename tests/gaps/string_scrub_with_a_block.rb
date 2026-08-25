# `String#scrub` ignores its BLOCK: zeo substitutes the default replacement
# where ruby yields each invalid byte run and splices the block's result.
# The no-argument and string-argument forms agree. (Found by the 2026-08-24
# probe sweep.)
p "a\xFFb".scrub { |c| "<#{c.unpack1('H*')}>" }
p "\xC3(ok".scrub { |c| "[#{c.bytes.join(',')}]" }
