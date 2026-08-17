la = lambda { return 5 }
p la.call
def m; pr = proc { return 7 }; pr.call; 9; end
p m
puts "still here"
pr2 = proc { |x| x * 2 }
p pr2.call(3)
