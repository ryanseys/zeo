p nil.singleton_class
p true.singleton_class
p false.singleton_class
begin
  1.singleton_class
rescue TypeError => e
  p e.class
end
