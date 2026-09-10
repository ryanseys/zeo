# `rand(3.5)` is an Integer in 0...3.
p rand(3.5).class
p (0...3).include?(rand(3.5))
__END__
Integer
true
