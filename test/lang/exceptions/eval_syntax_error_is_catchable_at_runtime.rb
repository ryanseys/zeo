bad = "1 +"
begin
  eval(bad)
rescue SyntaxError
  puts "caught"
end
__END__
caught
