begin
  Integer("zz")
rescue ArgumentError => e
  p e.backtrace
end

begin
  Hash([[1, 2]])
rescue TypeError => e
  p e.backtrace
end
