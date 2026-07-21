# Hash#sum without a block on a variable receiver: an empty hash returns the
# init value (0 by default); a non-empty hash raises TypeError (init + [k,v]).
h = {}
p h.sum
p h.sum(5)
g = { 1 => 2 }
begin
  g.sum
rescue => e
  p e.class
end
