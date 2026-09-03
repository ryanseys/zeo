# `Kernel#raise` with more than 3 positional arguments (appnexusapi passes
# a message twice) or with only `cause:` is a RUNTIME ArgumentError: the
# call evaluates its arguments first and raises only when reached, so the
# program still loads and an untaken branch never complains.
def never_reached
  raise RuntimeError, "a", "b", ["c"] if false
  :loadable
end
p never_reached

side_effects = []
begin
  raise RuntimeError, side_effects << :msg, side_effects << :bt, side_effects << :extra
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end
p side_effects

begin
  raise(cause: RuntimeError.new("x"))
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end
puts "still running"
__END__
:loadable
ArgumentError: wrong number of arguments (given 4, expected 0..3)
[:msg, :bt, :extra]
ArgumentError: only cause is given with no arguments
still running
