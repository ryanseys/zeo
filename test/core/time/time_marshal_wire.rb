# Time's marshal wire format, byte-for-byte (time.c time_mdump/mload): the
# 8-byte bit-packed UTC civil words, the year-extension tail, and the
# nano/submicro/offset/zone instance variables beside them.
def hex(s) = s.unpack1("H*")

p hex(Marshal.dump(Time.utc(2020, 1, 2, 3, 4, 5)))
p hex(Marshal.dump(Time.new(2020, 1, 2, 3, 4, 5, 3600)))
p hex(Marshal.dump(Time.utc(2020, 1, 2, 3, 4, 5.123456789r)))
p hex(Marshal.dump(Time.utc(99_999, 1, 1)))
p hex(Marshal.dump(Time.utc(1500, 6, 15, 12)))

# The private pair itself.
p Time.utc(2020, 1, 2, 3, 4, 5).send(:_dump).unpack1("H*")
p Time.private_instance_methods(false).include?(:_dump)

# Round-trips preserve instant, zone mode, and sub-second exactness.
t = Marshal.load(Marshal.dump(Time.utc(2020, 1, 2, 3, 4, 5)))
p [t, t.utc?, t.zone]
t2 = Marshal.load(Marshal.dump(Time.new(2020, 1, 2, 3, 4, 5, 3600)))
p [t2, t2.utc_offset, t2.zone, t2.utc?]
t3 = Marshal.load(Marshal.dump(Time.utc(2020, 1, 2, 3, 4, 5.123456789r)))
p [t3.nsec, t3.subsec]
t4 = Marshal.load(Marshal.dump(Time.utc(99_999, 1, 1)))
p [t4.year, t4.utc?]
t5 = Marshal.load(Marshal.dump(Time.utc(1500, 6, 15, 12)))
p [t5.year, t5.hour]

# Cross-loading CRuby's own canonical bytes (a 4.0.6 dump of
# 2020-01-02 03:04:05 UTC).
bytes = ["040849753a0954696d650d43001ec000005010063a097a6f6e65492208555443063a064546"].pack("H*")
p Marshal.load(bytes)

# `_load` with garbage refuses with CRuby's message.
begin
  Time.send(:_load, "short")
rescue TypeError => e
  puts e.message
end
__END__
"040849753a0954696d650d43001ec000005010063a097a6f6e65492208555443063a064546"
"040849753a0954696d650d42001e8000005010073a0b6f66667365746902100e3a097a6f6e6530"
"040849753a0954696d650d43001ec040e25110093a0d6e616e6f5f6e756d690215033a0d6e616e6f5f64656e69063a0d7375626d6963726f220778903a097a6f6e65492208555443063a064546"
"040849753a0954696d651020c0ffff0000000007347f063a097a6f6e65492208555443063a064546"
"040849753a0954696d6510ec1500c000000000079001063a097a6f6e65492208555443063a064546"
"43001ec000005010"
true
[2020-01-02 03:04:05 UTC, true, "UTC"]
[2020-01-02 03:04:05 +0100, 3600, nil, false]
[123456789, (123456789/1000000000)]
[99999, true]
[1500, 12]
2020-01-02 03:04:05 UTC
marshaled time format differ
