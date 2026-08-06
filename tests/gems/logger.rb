# `require "logger"` -- the vendored gem loads and Logger constructs. Covers a
# `module` nested in a `File.open { }` block (log_device.rb), `&.` with keyword
# arguments, `require "fiber"`, `File::BINARY` and `File.new(fd, path:)`.
require "logger"
p Logger.new(nil).class
