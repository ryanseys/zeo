# ripper is a CRuby C extension (exposes CRuby's own parser for Ruby source)
# that zeo has no native implementation of -- `require "ripper"` raises
# LoadError. Even if vendored as pure Ruby, it fundamentally depends on
# CRuby's own parser internals, so it would need a from-scratch
# implementation rather than a straight port.
require "ripper"
p defined?(Ripper)
