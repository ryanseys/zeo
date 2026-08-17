class C
  define_method(:inc) { |a, b = 5| a + b }
end
p C.new.inc(1, 9)
p C.new.inc(1)
class D
  define_method(:two) { |a, b| [a, b] }
end
r = (D.new.two(1) rescue $!.class); p r
p D.new.two(1, 2)
