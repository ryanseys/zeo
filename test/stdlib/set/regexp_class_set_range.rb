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
__END__
["[a-\\d]", :RegexpError]
["[\\d-z]", :RegexpError]
["[\\w-z]", :RegexpError]
["[a-\\w]", :RegexpError]
["[\\d-9]", :RegexpError]
["[[:alpha:]-z]", :RegexpError]
["[\\s-z]", :RegexpError]
["[^\\d-z]", :RegexpError]
["[-\\w]", ["-", "a", "5"]]
["[\\w-]", ["-", "a", "5"]]
["[-\\d-]", ["-", "5"]]
["[[:alpha:]-]", ["a", "-"]]
["[-[:digit:]]", ["-", "5"]]
["[\\d\\-z]", ["0", "-", "z"]]
["[-a]", ["-", "a"]]
["[a-]", ["-", "a"]]
["[a-z]", ["a", "m", "z"]]
["[0-9\\w]", ["0", "a"]]
["[\\w0-9]", ["0", "a"]]
["[\\d\\w]", ["0", "a"]]
["[[:alpha:]_]", ["a", "_"]]
["[^\\w-]", ["!"]]
