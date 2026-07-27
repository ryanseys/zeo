# continuation is a CRuby C extension (Kernel#callcc, deprecated in favor of
# Fiber) that zeo has no native implementation of -- `require "continuation"`
# raises LoadError.
require "continuation"
p defined?(Kernel.callcc)
