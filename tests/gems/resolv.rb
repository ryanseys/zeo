# `require "resolv"` -- the vendored gem loads. Covers a local assigned inside a
# parameter default (`def initialize(r = (arg_not_set = true; nil))`) and the
# ambient `RbConfig` every Ruby process has before its first line.
require "resolv"
p defined?(Resolv)
