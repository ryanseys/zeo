# A shorthand and a POSIX bracket each name a SET rather than a character, so
# neither can open or close a range in a class. Read as characters the class
# held something else entirely and said nothing about it: `[a-\d]` was `[a-d]`,
# four letters, and `[\d-z]` was the digits plus `-` plus `z`. CRuby reports
# the `-` instead. (ported from mruby-regexp 06ed794af)
def show(pat, chars)
  re = Regexp.new(pat)
  p [pat, chars.select { |ch| ch =~ re }]
rescue RegexpError
  p [pat, :RegexpError]
end

# a set at either end of a range
show('[a-\d]', ["a", "b", "d", "5", "-"])
show('[\d-z]', ["0", "-", "z"])
show('[\w-z]', ["a", "_", "-", "z"])
show('[a-\w]', ["a", "w", "-"])
show('[\d-9]', ["0", "9", "-"])
show('[[:alpha:]-z]', ["a", "-", "z"])
show('[\s-z]', [" ", "-", "z"])
show('[^\d-z]', ["0", "-", "z"])

# ...and every shape that must keep working: a `-` at either edge of the class
# is a member, and an escaped one is a member anywhere
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
