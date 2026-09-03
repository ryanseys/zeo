# %f/%e/%g print Inf/-Inf/NaN (Ruby's casing), not C's lowercase inf.

puts format("%.3f", Float::INFINITY)
puts format("%f", -Float::INFINITY)
puts format("%.2f", Float::NAN)
puts format("%e", Float::INFINITY)
__END__
Inf
-Inf
NaN
Inf
