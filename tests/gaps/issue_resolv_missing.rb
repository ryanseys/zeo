# resolv is vendored under gems/ now, but `require "resolv"` fails to compile:
# resolv.rb:87 assigns `arg_not_set` inside a conditional arm and reads it as a
# later sibling statement, and a local classified as Shadowed gets its `let`
# from the write site only -- so the read references an unbound identifier.
require "resolv"
p defined?(Resolv)
