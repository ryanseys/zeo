[[1000000, :a], [500000, :b]].each do |odds, name|
  r = rand(odds)
  p r.class
  p r >= 0
  p(r == 0 || r > 0)
end
