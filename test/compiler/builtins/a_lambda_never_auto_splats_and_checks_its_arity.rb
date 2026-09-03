strict = ->(a, b) { [a, b] }
p strict.call(1, 2)
begin
  strict.call([1, 2])
rescue ArgumentError => e
  puts "ArgumentError"
end
__END__
[1, 2]
ArgumentError
