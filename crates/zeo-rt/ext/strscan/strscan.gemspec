# The Ruby half of `strscan`. Its native half is the statically linked
# `ext-strscan` module in zeo-rt, beside it here -- the same
# split CRuby makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "strscan"
  s.version = "3.1.6"
  s.summary = "StringScanner's Ruby-level surface."
  s.require_paths = ["lib"]
end
