def show(label)
  puts format("%-22s %s", label, (yield).inspect)
rescue => e
  puts format("%-22s %s: %s", label, e.class, e.message.to_s[0, 44])
end

show("p pointer size")    { ["x"].pack("p").bytesize }
show("P pointer size")    { ["x"].pack("P").bytesize }
show("p round trip")      { ["hello"].pack("p").unpack1("p") }
show("P round trip")      { ["hello"].pack("P5").unpack1("P5") }
show("p nil")             { [nil].pack("p").bytes }
