n = -5
if n < 0
  puts :negative
elsif n == 0
  puts :zero
else
  puts :positive
end

n2 = 0
result = if n2 < 0
  :negative
elsif n2 == 0
  :zero
else
  :positive
end
puts result
__END__
negative
zero
