# objspace is a CRuby C extension. `require "objspace"` loads without a
# LoadError, but its actual introspection methods (e.g.
# ObjectSpace.memsize_of) aren't implemented.
require "objspace"
p ObjectSpace.respond_to?(:memsize_of)
