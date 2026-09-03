# `require "csv"` -- the vendored gem loads, which brings `CSV.parse` and
# `CSV.generate` with it.
require "csv"
puts "loaded"
__END__
loaded
