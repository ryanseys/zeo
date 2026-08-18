r = catch do |t|
  p t.class
  throw t, :done
end
p r
