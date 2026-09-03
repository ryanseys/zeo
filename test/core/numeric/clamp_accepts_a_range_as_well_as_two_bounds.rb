p 0.clamp(1..5)
p 9.clamp(1..5)
p 3.clamp(1..5)
p 0.clamp(1..)
p 99.clamp(..5)
p 9.clamp(1, 5)
begin
  9.clamp(1...5)
rescue ArgumentError => e
  puts "ArgumentError"
end
__END__
1
5
3
1
5
5
ArgumentError
