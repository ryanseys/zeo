begin
  Time.at({})
rescue TypeError => e
  p e.class
end
