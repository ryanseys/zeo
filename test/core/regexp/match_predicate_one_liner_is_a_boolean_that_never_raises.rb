if [1, 2] in [Integer, Integer]
  puts "matched"
end
if [1, "x"] in [Integer, Integer]
  puts "should not print"
else
  puts "did not match"
end
__END__
matched
did not match
