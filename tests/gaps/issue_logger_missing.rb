# logger is vendored under gems/ now, but `require "logger"` still fails:
# log_device.rb:258 puts a `module PathAttr` inside a `File.open { }` block,
# and the analyze walk only registers a class/module at the top level, so
# codegen panics rather than compiling it.
require "logger"
p Logger.new(nil).class
