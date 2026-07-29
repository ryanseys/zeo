# The Ruby half of `nkf`. Its native half is the statically linked
# `ext-nkf` module in zeo-rt, joined to this gem by name -- the same split
# CRuby makes between rubylibdir (.rb) and archdir (.so). `kconv` is this
# gem's second require, exactly as upstream nkf ships lib/kconv.rb.
Gem::Specification.new do |s|
  s.name = "nkf"
  s.version = "0.3.0"
  s.summary = "Network Kanji Filter and the Kconv wrapper."
  s.require_paths = ["lib"]
end
