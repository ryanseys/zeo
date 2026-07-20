class Even
  def ===(n); n.even?; end
end
e = Even.new

puts(case 4 when e then "when-obj" else "when-missed" end)
puts(case 3 when e then "when-obj" else "when-missed" end)

r = case 4
    in ^e then "pin"
    else "pin-missed"
    end
puts r

puts [1, 2, 3, 4].grep(e).inspect
puts (1..4).lazy.grep(e).to_a.inspect

class Trip; end
def Trip.===(n); n % 3 == 0; end
puts(case 9 when Trip then "singleton" else "singleton-missed" end)

puts(case 5 when Integer then "builtin-class" else "no" end)
puts(case "hi" when /h/ then "regexp" else "no" end)
puts(case 3 when 1..5 then "range" else "no" end)
puts(case 2 when 1, 2, 3 then "listed" else "no" end)
cands = [7, 8]
puts(case 8 when *cands then "splat" else "no" end)
