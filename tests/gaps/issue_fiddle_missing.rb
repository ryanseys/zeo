# fiddle is a CRuby C extension (libffi-based dynamic library binding) that
# zeo has no native implementation of -- `require "fiddle"` raises
# LoadError.
require "fiddle"
p Fiddle::TYPE_INT
