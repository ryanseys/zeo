# Module#const_defined? only supports the single-argument form -- passing the
# second `inherit` boolean argument raises NoMethodError as if the method
# didn't exist at all, instead of being accepted with its normal meaning
# (whether to also search ancestors). This breaks bare `require "uri"`: the
# vendored gem's top level calls `RFC2396_REGEXP::PATTERN.const_defined?(sym,
# false)` and crashes before any URI functionality can be used.
module Foo
  X = 1
end
p Foo.const_defined?(:X, false)
