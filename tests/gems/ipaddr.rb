require "ipaddr"

v4 = IPAddr.new("192.168.1.10")
puts v4.to_s
puts v4.ipv4?
puts v4.ipv6?

net = IPAddr.new("192.168.1.0/24")
puts net.to_s
puts net.prefix
puts net.include?(v4)
puts net.include?(IPAddr.new("10.0.0.1"))
puts net.to_range.first.to_s
puts net.to_range.last.to_s

v6 = IPAddr.new("2001:db8::1")
puts v6.ipv6?
puts v6.to_s
puts v6.to_string

puts IPAddr.new("192.168.1.10").mask(16).to_s
puts (IPAddr.new("10.0.0.1") == IPAddr.new("10.0.0.1"))
puts IPAddr.new("192.168.1.10").succ.to_s

begin
  IPAddr.new("not-an-address")
rescue IPAddr::InvalidAddressError => e
  puts "invalid: #{e.class}"
end
