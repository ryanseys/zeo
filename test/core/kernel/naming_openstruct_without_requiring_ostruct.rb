# A clean NameError, the same path as any undefined constant, rescued so the
# output is deterministic.
begin
  o = OpenStruct.new(a: 1)
  p o.a
rescue NameError => e
  puts "NameError: #{e.message}"
end

begin
  x = [1, "s"].first
  p x.no_such_method
rescue NoMethodError
  puts "NoMethodError caught"
end
puts "ok"
__END__
NameError: uninitialized constant OpenStruct
NoMethodError caught
ok
