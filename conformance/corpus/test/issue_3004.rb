begin
  raise "x"
rescue => e
  p e.frozen?
end
begin
  raise ArgumentError, "y"
rescue ArgumentError => e
  p e.frozen?
  p e.nil?
end
ex = RuntimeError.new("z")
p ex.frozen?
