# `Range#===` IS `#cover?` -- what makes `case x when 1..50` actually
# match (previously it fell to structural equality and silently never
# did). Endpoint semantics oracle-verified incl. exclusive ends, floats,
# and incomparable-subject false.

case 42
when 1..50 then puts "hit"
else puts "miss"
end

x = [5.5].first
puts (1..5) === x
puts (1..6) === x
puts (1...5) === 5
puts (1..5).cover?(3)
puts (1..5).include?("x")
case "c"
when "a".."f" then puts "letter"
end
__END__
hit
false
true
false
true
false
letter
