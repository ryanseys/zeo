# OpenStruct without `require "ostruct"` must raise a clean NameError (the same
# path as any undefined constant), not fail C compilation. The general shape --
# an unresolved call in `p`'s argument position -- must compile to its diverging
# NoMethodError/NameError raise. Rescue so the output is deterministic.
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
