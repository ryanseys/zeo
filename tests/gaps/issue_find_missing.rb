# find is a pure-Ruby default gem (no C extension) that isn't vendored under
# gems/ -- `require "find"` raises LoadError.
require "find"
p Find.respond_to?(:find)
