# A computed require makes this package's whole load path compile as
# UNITS, which is what puts patch.rb in a unit rather than a splice.
require ENV["ZEO_NEVER_SET"] if ENV["ZEO_NEVER_SET"]
puts "before\t#{require("set")}"
eval File.read(File.join(__dir__, "patch.rb")), nil, "patch.rb"
puts "after\t#{require("tmpdir")}"
