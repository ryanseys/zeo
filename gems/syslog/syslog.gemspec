# The Ruby half of `syslog`. Its native half is the statically linked
# `ext-syslog` module in zeo-rt, joined to this gem by name -- the same split
# CRuby makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "syslog"
  s.version = "0.4.0"
  s.summary = "Syslog's constant submodules and Syslog::Logger."
  s.require_paths = ["lib"]
end
