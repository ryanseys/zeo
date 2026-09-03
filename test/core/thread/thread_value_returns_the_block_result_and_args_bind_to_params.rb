v = Thread.new(20, 22) { |a, b| a + b }.value
puts v
__END__
42
