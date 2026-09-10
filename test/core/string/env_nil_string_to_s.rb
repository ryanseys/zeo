v = ENV["PROBE_NO_SUCH_1664"].to_s
puts v == ""
puts v.length
puts v.nil?
w = ENV["PROBE_NO_SUCH_1664"]
puts w.nil?
ENV["PROBE_T_1664"] = "val"
puts ENV["PROBE_T_1664"].to_s
ENV["PROBE_T_1664"] = nil
puts ENV["PROBE_T_1664"].nil?
puts "x#{ENV["PROBE_NO_SUCH_1664"]}y"
__END__
true
0
false
true
val
true
xy
