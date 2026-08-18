P = Data.define(:x)
begin
  P.new(y: 1)
rescue ArgumentError => e
  p e.message
end
begin
  P.new
rescue ArgumentError => e
  p e.message
end
