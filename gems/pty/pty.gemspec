# The Ruby half of `pty`. Its native half is the statically linked
# `ext-pty` module in zeo-rt, joined to this gem by name -- the same split
# CRuby makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "pty"
  s.version = "0.5.9"
  s.summary = "PTY's exception class."
  s.require_paths = ["lib"]
end
