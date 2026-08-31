Gem::Specification.new do |s|
  s.name = "puretop"
  s.version = "0.3.1"
  s.summary = "the root of the parity probe's dependency graph"
  s.authors = ["zeo"]
  s.license = "MIT"
  s.required_ruby_version = ">= 3.1"
  s.files = ["lib/puretop.rb", "exe/puretop"]
  s.executables = ["puretop"]
  s.bindir = "exe"
  s.require_paths = ["lib"]
  s.add_dependency "pureleaf", "~> 1.2"
end
