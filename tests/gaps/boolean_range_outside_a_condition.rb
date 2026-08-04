# A `cond..cond` expression is only a flip-flop in a conditional context;
# anywhere else it builds a Range, and a Range of booleans raises
# ArgumentError ("bad value for range") the moment it is compared. zeo treats
# the block-body form as a flip-flop and happily yields values. The
# if-modifier control below is a REAL flip-flop and works on both sides.
picked = []
%w[a START b END c].each do |l|
  picked << l if (l == "START")..(l == "END")
end
puts picked.inspect

begin
  r = (1..20).select { |i| (i % 5 == 1)..(i % 5 == 3) }
  puts r.inspect
rescue ArgumentError => e
  puts "select: #{e.message}"
end
