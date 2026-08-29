# The Ruby half of `io-console`. Its native half is the statically linked
# `ext-io-console` module in zeo-rt, beside it here -- the same split CRuby
# makes between rubylibdir (.rb) and archdir (.so).
#
# Upstream lays the native half out as `ext/io/console/console.c`, and this
# tree keeps that path so the two read side by side.
Gem::Specification.new do |s|
  s.name = "io-console"
  s.version = "0.8.2"
  s.summary = "Console interface, the raw/cooked terminal modes and the cursor escapes."
  s.require_paths = ["lib"]
end
