# resolv is a pure-Ruby default gem (a DNS resolver, no C extension) that
# isn't vendored under gems/ -- `require "resolv"` raises LoadError.
require "resolv"
p defined?(Resolv)
