p(/(?:(?!a))*b?/.match("b").to_a)
p(/a(?:(?<=a))*b?/.match("ab").to_a)
