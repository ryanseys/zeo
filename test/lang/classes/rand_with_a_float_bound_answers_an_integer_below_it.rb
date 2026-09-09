# `rand(3.5)` is an Integer in 0...3.
# (spinel issue #2868)
p rand(3.5).class
p (0...3).include?(rand(3.5))
__END__
Integer
true
