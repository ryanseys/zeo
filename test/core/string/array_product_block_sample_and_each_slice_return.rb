# product's block form yields each tuple and returns self; sample(-n) has
# its own message; each_slice/each_cons block forms return the receiver.

r = []
ret = [1, 2].product([3, 4]) { |t| r << t }
p r
p ret
p([1, 2, 3].each_slice(2) { |s| })
p([1, 2, 3].each_cons(2) { |s| })
begin
  [1, 2].sample(-1)
rescue ArgumentError => e
  p e.message
end
__END__
[[1, 3], [1, 4], [2, 3], [2, 4]]
[1, 2]
[1, 2, 3]
[1, 2, 3]
"negative sample number"
