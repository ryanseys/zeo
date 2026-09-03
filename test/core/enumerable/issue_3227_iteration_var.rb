# Iteration-variable mutation (P6): the block binding holds the element
# handle, so in-place mutators reach the container's elements.
arr = [+"a", +"b"]
arr.each { |x| x << "!" }
p arr
s = +"s0"
lst = [s, +"t0"]
lst.each { |x| x.upcase! }
p lst
p s
ws = [+"x", +"y"]
ws.each_with_index { |w, i| w << i.to_s }
p ws
__END__
["a!", "b!"]
["S0", "T0"]
"S0"
["x0", "y1"]
