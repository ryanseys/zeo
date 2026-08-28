# A method called through a variable holding a Class value dispatches on the
# runtime class id, with one arm per class that defines the name. A candidate
# whose method cannot take this call's argument count is not a possible
# receiver HERE -- CRuby raises ArgumentError if you try -- but it used to veto
# the whole dispatch, so the call was refused at compile time (#4129).
#
# `Other` below is never called and never passed anywhere.
module Other
  def self.answer(a, b) = "other #{a}#{b}"
end

module Grammar
  def self.answer(a) = "grammar #{a}"
end

def read(thing)
  thing.answer("x")
end

p read(Grammar)

# Optional parameters make the acceptance a range, not an equality: the old
# test compared nrequired to argc, so a candidate with a trailing default
# called with the argument that fills it was refused too.
module Loose
  def self.tag(a, b = "d") = "loose #{a}#{b}"
end

module Tight
  def self.tag(a, b) = "tight #{a}#{b}"
end

def label(thing, *rest)
  rest.length == 1 ? thing.tag(rest[0]) : thing.tag(rest[0], rest[1])
end

p label(Loose, "1")
p label(Loose, "1", "2")
p label(Tight, "1", "2")

# The arm the call CAN reach still answers, and the arms it cannot are still
# reachable through their own arity.
def one(thing) = thing.answer("only")
p one(Grammar)

def two(thing) = thing.answer("a", "b")
p two(Other)

# Calling a class through a variable with an argument count its method cannot
# take raises ArgumentError, as CRuby does, rather than answering nil.
begin
  p one(Other)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
