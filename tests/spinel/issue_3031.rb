begin
  raise ArgumentError, "x"
rescue => e
  p e.class.superclass
end
begin
  raise TypeError, "y"
rescue => e
  p e.class.superclass
end
begin
  {}.fetch(:missing)
rescue KeyError => e
  p e.class.superclass
end
p ArgumentError.superclass
