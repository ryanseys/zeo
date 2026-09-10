# An exception object reaching a rescue clause is mutable, whether raised by message or by class.
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
__END__
false
false
false
false
