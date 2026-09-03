f = ->(x, y) { x + y }
begin
  f.call(1)
rescue ArgumentError => e
  puts "caught: #{e.message}"
end
puts f.call(1, 2)
__END__
caught: wrong number of arguments (given 1, expected 2)
3
