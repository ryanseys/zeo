# reduce(nil) -- folding with no natural identity
p [1, 2, 3].reduce(nil) { |acc, t| acc.nil? ? t : t + acc }
p [1, 2, 3].reduce(nil) { |acc, t| acc.nil? ? t : acc + t }
p ["a", "b"].reduce(nil) { |acc, t| acc.nil? ? t : acc + t }
p [1, 2, 3].reduce(nil) { |acc, t| next t if acc.nil?; acc + t }
p [1, 2, 3].inject(nil)  { |acc, t| acc.nil? ? t : acc + t }
p [].reduce(nil) { |acc, t| acc.nil? ? t : acc + t }
p [1, 2, 3].reduce(0) { |acc, t| acc + t }

acc = nil
[1, 2, 3].each { |t| acc = acc.nil? ? t : acc + t }
p acc

x = [nil, Rational(2)].last
p Rational(1) / x
y = [nil, 4].last
p 8 / y
