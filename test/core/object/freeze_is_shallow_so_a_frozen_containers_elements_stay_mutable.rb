inner = [1, 2]
b = [inner]
b.freeze
inner[0] = 99
puts inner[0]
puts b.frozen?
__END__
99
true
