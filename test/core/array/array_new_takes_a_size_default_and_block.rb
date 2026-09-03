# The block form has to reach the runtime allocator, so `Array.new`
# joins the block-keeping set in lowering -- routing it through
# `HirNode::New` (which has no block slot) silently answered nils.

p Array.new(3) { |i| i * 2 }
p Array.new(2, "x")
p Array.new(3)
p Array.new
__END__
[0, 2, 4]
["x", "x"]
[nil, nil, nil]
[]
