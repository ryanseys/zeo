# A gem spinel ships. The manifest is a real gemspec, parsed statically by
# `parse::gemspec` -- the same reader that handles an installed gem's
# serialized spec, so there is one format and one code path.
Gem::Specification.new do |s|
  s.name = "optparse"
  s.version = "0.8.1"
  s.summary = "Command-line option analysis, as a spinel-compiled subset."
  s.require_paths = ["lib"]
end
