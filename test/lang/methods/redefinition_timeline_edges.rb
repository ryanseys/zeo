# The positional-redefinition timeline (`analyze::redefs`) beyond the two
# promoted gap shapes: each call must see the body installed AT THAT MOMENT.

# Three bodies across three sites -- every window observed.
class Tri
  def gen = 1
end
puts Tri.new.gen

class Tri
  def gen = 2
end
puts Tri.new.gen

class Tri
  def gen = 3
end
puts Tri.new.gen

# A subclass instance follows the SUPERCLASS's timeline (the overlay
# propagates down the ancestry).
class TriKid < Tri; end
puts TriKid.new.gen

# The redefinition changes ARITY: the pre-reopen body takes an argument,
# the post-reopen one refuses it.
class Ar
  def m(x) = "one:#{x}"
end
puts Ar.new.m(5)

class Ar
  def m = "zero"
end
puts Ar.new.m
begin
  Ar.new.m(5)
rescue ArgumentError => e
  puts e.message
end

# `method(:name)` reflection resolves positionally too.
class Refl
  def word = "early"
end
m = Refl.new.method(:word)
puts m.call

class Refl
  def word = "late"
end
puts Refl.new.method(:word).call
# A Method object captured BEFORE the reopen: CRuby's Method is a snapshot
# of the entry it was taken from.
puts m.call

# An attr reader superseded by a hand-written one ACROSS sites, with a
# reader call in the window.
class Acc
  attr_accessor :v
end
a = Acc.new
a.v = 10
puts a.v

class Acc
  def v = "handwritten"
end
puts a.v

# A yield-taking body superseded by a non-yielding one.
class Blk
  def run = yield(1)
end
puts(Blk.new.run { |x| x + 1 })

class Blk
  def run = "no yield"
end
puts Blk.new.run
__END__
1
2
3
3
one:5
zero
wrong number of arguments (given 1, expected 0)
early
late
early
10
handwritten
2
no yield
