def horner(coeffs, x)
  coeffs.inject(0) { |acc, c| acc * x + c }
end
poly = [2, -3, 4, -5]
puts "int:   #{horner(poly, 2)}"
puts "rat:   #{horner(poly, Rational(1, 2))}"
puts "float: #{horner(poly, 1.5)}"
p horner([1, 1], Rational(3, 4))
p [1.5, 2.5].inject(0) { |a, x| a + x }
