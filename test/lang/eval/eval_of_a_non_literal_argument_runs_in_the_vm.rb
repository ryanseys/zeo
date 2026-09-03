# A non-literal source is not rejected at compile time; it runs
# through the runtime `eval`. (`y` is interpolated INTO the source string, not
# referenced inside the eval.)

y = 40
puts eval("#{y} + 2")
__END__
42
