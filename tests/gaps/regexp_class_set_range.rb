def show(pat, chars)
  re = Regexp.new(pat)
  p [pat, chars.select { |ch| ch =~ re }]
rescue RegexpError
  p [pat, :RegexpError]
end

show('[a-\d]', ["a", "b", "d", "5", "-"])
show('[\d-z]', ["0", "-", "z"])
show('[\w-z]', ["a", "_", "-", "z"])
show('[a-\w]', ["a", "w", "-"])
show('[\d-9]', ["0", "9", "-"])
show('[[:alpha:]-z]', ["a", "-", "z"])
show('[\s-z]', [" ", "-", "z"])
show('[^\d-z]', ["0", "-", "z"])

show('[-\w]', ["-", "a", "5"])
show('[\w-]', ["-", "a", "5"])
show('[-\d-]', ["-", "5", "a"])
show('[[:alpha:]-]', ["a", "-", "5"])
show('[-[:digit:]]', ["-", "5", "a"])
show('[\d\-z]', ["0", "-", "z", "m"])
show('[-a]', ["-", "a"])
show('[a-]', ["-", "a"])
show('[a-z]', ["a", "m", "z", "-"])
show('[0-9\w]', ["0", "a", "-"])
show('[\w0-9]', ["0", "a", "-"])
show('[\d\w]', ["0", "a", "-"])
show('[[:alpha:]_]', ["a", "_", "-"])
show('[^\w-]', ["-", "a", "5", "!"])
