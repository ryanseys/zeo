begin
  "Hello" =~ 0
rescue NoMethodError => e
  p e.class
end
p("Hello" =~ nil)
begin
  :abc =~ [1]
rescue NoMethodError => e
  p e.class
end
p(:abc =~ nil)
__END__
NoMethodError
nil
NoMethodError
nil
