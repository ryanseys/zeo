# logger is a pure-Ruby default gem (no C extension) that isn't vendored
# under gems/ -- `require "logger"` raises LoadError.
require "logger"
p Logger.new(nil).class
