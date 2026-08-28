# The Ruby half of `zlib`. Its native half is the statically linked
# `ext-zlib` module in zeo-rt, beside it here -- the same split
# CRuby makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "zlib"
  s.version = "3.2.3"
  s.summary = "Zlib's exception hierarchy."
  s.require_paths = ["lib"]
end
