# The csv stdlib gem isn't vendored at all -- `require "csv"` raises
# LoadError instead of loading CSV.parse/CSV.generate support.
require "csv"
puts "loaded"
