# Time value in string interpolation calls #to_s (KieranP #2358)
t = Time.at(0).utc
puts "at '#{t}'"
t2 = Time.at(3661).utc
puts "T=#{t2} end"
puts "#{Time.at(0).utc}"
__END__
at '1970-01-01 00:00:00 UTC'
T=1970-01-01 01:01:01 UTC end
1970-01-01 00:00:00 UTC
