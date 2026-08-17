Line = Struct.new(:number, :label)
lines = []
"a\nb\n".each_line.with_index do |raw, number|
  lines << Line.new(number, raw.to_s)
end
p lines.map(&:number)
p Line.new(99, "x").number

"a\nb\n".each_line.with_index { |l, i| p [l, i] }
"ab".each_byte.with_index { |b, i| p [b, i] }
"ab".each_char.with_index { |ch, i| p [ch, i] }
"a\nb\n".each_line.with_index(10) { |l, i| p [l, i] }
"a|b".each_line("|").with_index { |l, i| p [l, i] }
p("a\nb\n".each_line.with_index.to_a)
p("ab".each_byte.with_index.map { |b, i| b + i })

"a\nb\n".each_line.with_index do |raw, number|
  p number.class
end
