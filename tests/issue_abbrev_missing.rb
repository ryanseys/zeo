# abbrev is a pure-Ruby default gem (no C extension) that isn't vendored
# under gems/ -- `require "abbrev"` raises LoadError.
require "abbrev"
p Abbrev.respond_to?(:abbrev)
