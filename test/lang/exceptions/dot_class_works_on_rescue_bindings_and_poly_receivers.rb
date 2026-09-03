begin
  raise "boom"
rescue => e
  puts e.class
end

mixed = [1, "two", nil]
mixed.each do |v|
  puts v.class
end
__END__
RuntimeError
Integer
String
NilClass
