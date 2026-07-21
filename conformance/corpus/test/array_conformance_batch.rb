# Array conformance: rfind, fetch_values (+IndexError), Array.try_convert,
# empty-literal sample, deconstruct/to_ary identity, prepend/append aliases.
p [1, 2, 3, 4].rfind { |x| x.even? }
p [1, 2, 3, 4].rfind { |x| x > 10 }
p [1, 2, 3].fetch_values(0, 2)
p [1, 2, 3].fetch_values(0, -1)
begin
  [1, 2, 3].fetch_values(0, 5)
rescue IndexError => e
  puts "IE: #{e.message}"
end
p Array.try_convert([1, 2])
p Array.try_convert("x")
p [].sample
p [1, 2, 3].deconstruct
p [1, 2].to_ary
p [].to_ary
p [].deconstruct
p [1, 2].prepend(0)
a = [1, 2]; a.append(nil); p a
