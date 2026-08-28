# A miss on a specialized container hands back the element type's own C nil --
# SP_INT_NIL, a NULL string -- and the receiver arms read it as an ordinary
# value. #4070 spelled the check out per name (to_s, inspect, to_i, to_f) and
# every name it did not reach kept answering: `h["zz"].succ` was
# -9223372036854775807, `h["zz"].bit_length` was 63, `s["zz"].hex` was 0.
#
# CRuby's nil answers only what NilClass defines. That is the exception list;
# everything else is a NoMethodError. This program walks both halves so a name
# added to one of the receiver chains has to say which half it is in.

def try
  yield
rescue NoMethodError
  "NoMethodError"
rescue => e
  e.class.to_s
end

n = { "a" => 1 }

# the names nil REFUSES on an int-typed miss
%w[succ pred next bit_length ceil floor round truncate ord size magnitude
   digits chr integer? finite? real imaginary numerator denominator].each do |m|
  puts m + ": " + try { n["zz"].send(m).inspect }.to_s
end

# the names nil ANSWERS
puts "to_i: " + n["zz"].to_i.inspect
puts "to_f: " + n["zz"].to_f.inspect
puts "to_r: " + n["zz"].to_r.inspect
puts "to_c: " + n["zz"].to_c.inspect
puts "to_s: " + n["zz"].to_s.inspect
puts "inspect: " + n["zz"].inspect.inspect
puts "nil?: " + n["zz"].nil?.inspect

# and the same names on a real value still work
puts "hit succ: " + n["a"].succ.inspect
puts "hit bit_length: " + n["a"].bit_length.inspect
puts "hit to_r: " + n["a"].to_r.inspect
puts "hit to_c: " + n["a"].to_c.inspect
puts "hit numerator: " + n["a"].numerator.inspect
puts "hit denominator: " + n["a"].denominator.inspect

s = { "a" => "ab" }

%w[b hex oct intern to_sym lines].each do |m|
  puts "s " + m + ": " + try { s["zz"].send(m).inspect }.to_s
end

puts "s to_i: " + s["zz"].to_i.inspect
puts "s to_s: " + s["zz"].to_s.inspect
puts "s nil?: " + s["zz"].nil?.inspect
puts "s hit hex: " + s["a"].hex.inspect
puts "s hit to_sym: " + s["a"].to_sym.inspect
puts "s hit lines: " + s["a"].lines.inspect

# an array miss carries the same sentinel
a = [7]
puts "a succ: " + try { a[9].succ.inspect }.to_s
puts "a to_i: " + a[9].to_i.inspect
puts "a hit succ: " + a[0].succ.inspect
