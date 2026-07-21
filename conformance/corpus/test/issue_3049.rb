inf = 1.0 / 0.0
nan = 0.0 / 0.0
begin
  x = rand(inf)
  puts "no-raise-inf"
rescue FloatDomainError => e
  puts e.message
end
begin
  rand(nan)
  puts "no-raise-nan"
rescue FloatDomainError => e
  puts e.message
end
puts rand(0.0).class
puts(rand(3.5) >= 0)
srand(42)
r = rand(5.9)
puts(r >= 0 && r <= 5)
