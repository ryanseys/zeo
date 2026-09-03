case [1, 2, 3, 4, 5]
in [*, Integer => a, Integer => b, *]
  puts "#{a} #{b}"
end
case [10, 20, 30]
in [*, 99, *]
  puts "found 99"
else
  puts "not found"
end
__END__
1 2
not found
