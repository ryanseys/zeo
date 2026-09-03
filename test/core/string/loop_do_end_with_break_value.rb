i = 0
result = loop do
  i += 1
  break i * 10 if i == 3
end
puts result
__END__
30
