a = [1, 2]
case 1
when *a then puts "hit"
else puts "miss"
end
case 9
when *a then puts "hit"
else puts "miss"
end
case 5
when 3, *a, 5 then puts "mixed hit"
else puts "mixed miss"
end
kinds = [Integer, String]
case "s"
when *kinds then puts "kind hit"
end
none = []
case 1
when *none then puts "never"
else puts "empty ok"
end
__END__
hit
miss
mixed hit
kind hit
empty ok
