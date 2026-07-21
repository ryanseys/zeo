t = Thread.new { 21 * 2 }
puts t.value

s = Thread.new { "hi" + "!" }
puts s.value
