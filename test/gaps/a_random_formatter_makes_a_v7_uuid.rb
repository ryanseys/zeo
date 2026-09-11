# `uuid_v7` on the random formatter.
require "random/formatter"
r = Random.new
p r.respond_to?(:uuid_v7)
u = r.uuid_v7
p u.length, u[14], u.count("-")
p r.uuid_v7(extra_timestamp_bits: 12).length
begin
  r.uuid_v7(extra_timestamp_bits: 13)
rescue ArgumentError => e
  puts e.message
end
__END__
true
36
"7"
4
36
extra_timestamp_bits must be in 0..12
