i = 0
sum = 0
while i < 10
  i += 1
  next if i == 5
  break if i == 8
  sum += i
end
puts sum
puts i
__END__
23
8
