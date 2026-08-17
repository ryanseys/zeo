begin
  "x".freeze << "y"
rescue FrozenError => e
  p e.backtrace
end
