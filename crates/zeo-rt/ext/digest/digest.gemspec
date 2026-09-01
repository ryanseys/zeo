# The Ruby half of `digest` -- ruby's own lib/digest.rb, vendored verbatim.
# Its native half is the statically linked `ext-digest` module in zeo-rt,
# beside it here -- the same split CRuby makes between rubylibdir (.rb) and
# archdir (.so).
Gem::Specification.new do |s|
  s.name = "digest"
  s.version = "3.2.1"
  s.summary = "The Digest framework's Ruby half."
  s.require_paths = ["lib"]
end
