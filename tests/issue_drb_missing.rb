# drb is a pure-Ruby default gem (distributed Ruby, no C extension) that
# isn't vendored under gems/ -- `require "drb"` raises LoadError.
require "drb"
p defined?(DRb)
