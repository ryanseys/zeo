# The Ruby half of `openssl`. Its native half is the statically linked
# `ext-openssl` module in zeo-rt (the official rust-openssl bindings over a
# vendored OpenSSL 3.x), joined to this gem by name -- the same split CRuby
# makes between rubylibdir (.rb) and archdir (.so).
Gem::Specification.new do |s|
  s.name = "openssl"
  s.version = "4.0.2"
  s.summary = "OpenSSL's exception hierarchy and Ruby-space helpers."
  s.require_paths = ["lib"]
end
