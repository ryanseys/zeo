# The pure-Ruby gem tier's StringScanner: a zeo-authored port of strscan's
# own lib/strscan/truffleruby.rb (3.1.8), that engine's primitives replaced
# with plain Ruby over this runtime's String and Regexp. Served when a
# build omits `ext-strscan`; the Rust ext stays the default build's
# implementation. Differentially matched against the C extension under
# CRuby (the -I harness) and against the oracle under zeo.
Gem::Specification.new do |s|
  s.name = "strscan"
  s.version = "3.1.8"
  s.summary = "Provides lexical scanning operations on a String."
  s.require_paths = ["lib"]
end
