1.step(by: 2, to: 10) { |i| print i, " " }
puts
1.step(10, 2) { |i| print i, " " }
puts
1.step(to: 5) { |i| print i, " " }
puts
10.step(by: -3, to: 1) { |i| print i, " " }
puts
__END__
1 3 5 7 9 
1 3 5 7 9 
1 2 3 4 5 
10 7 4 1 
