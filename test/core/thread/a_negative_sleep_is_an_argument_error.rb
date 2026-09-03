# `sleep(-1)` raises ArgumentError "time interval must not be negative";
# zeo raises TypeError with a conversion message. (Found by the 2026-08-24
# probe sweep.)
begin
  sleep(-1)
rescue StandardError => e
  puts "#{e.class}: #{e.message}"
end
__END__
ArgumentError: time interval must not be negative
