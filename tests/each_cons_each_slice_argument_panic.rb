begin
  [1, 2, 3].each_cons("l")
rescue TypeError => e
  p e.class
end
begin
  [1, 2, 3].each_slice(:a)
rescue TypeError => e
  p e.class
end
