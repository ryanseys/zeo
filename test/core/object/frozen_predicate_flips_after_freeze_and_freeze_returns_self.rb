a = [1]
puts a.frozen?
b = a.freeze
puts a.frozen?
puts b.frozen?
__END__
false
true
true
