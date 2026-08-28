# The Ruby half of `prism`. Its native half is the statically linked
# `ext-prism` module in zeo-rt, joined to this gem by name -- the same split
# CRuby makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "prism"
  s.version = "1.9.0"
  s.summary = "A parser for the Ruby programming language."
  s.require_paths = ["lib"]
end
