# Regexp#match on a Regexp.new pattern has to accept a subject that arrives
# boxed: one call site handing it an untyped block parameter is enough to
# widen the method's parameter, and the subject then reaches the runtime as a
# boxed value rather than a C string.
RE = Regexp.new("\\A(?<id>\\d+)\\z")

def m(path) = RE.match(path)
def mp(path) = RE.match?(path)
def mat(path, pos) = RE.match(path, pos)

EMPTY = [].freeze
EMPTY.each { |path| p m(path) }
EMPTY.each { |path| p mp(path) }

p m("42")[:id]
p m("x").nil?
p mp("42")
p mp("x")
p mat("42", 0)[0]

# a boxed subject that really is a String still matches
subjects = ["42", "x"]
subjects.each { |s| p RE.match(s).nil? }
