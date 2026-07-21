s = GC.stat
puts s.key?("bytes")
puts s.key?("old_bytes")
puts s.key?("threshold")
puts s.key?("cycle")
puts s.key?("full_runs")
puts s["bytes"] >= 0
puts s["threshold"] > 0
GC.start
t = GC.stat
puts t["cycle"] >= s["cycle"]
puts "done"
