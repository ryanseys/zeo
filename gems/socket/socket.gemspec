# The Ruby half of `socket`. Its native half is the statically linked
# `ext-socket` module in zeo-rt, joined to this gem by name -- the same
# split CRuby makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "socket"
  s.version = "0.7.1"
  s.summary = "Socket's exception hierarchy."
  s.require_paths = ["lib"]
end
