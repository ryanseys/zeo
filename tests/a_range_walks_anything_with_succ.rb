# `Range#each` falls through to `succ` for ANY value that answers it --
# CRuby's `range_each` does exactly that test, which is what makes a Date
# range iterable and what any user class with `succ` and `<=>` gets free.
#
# Only three types had walks of their own (Integer, String, Symbol), so
# everything else answered `can't iterate from <Class>` -- including Date,
# which is the shape gems actually write.

require "date"

r = Date.new(2001, 1, 1)..Date.new(2001, 1, 3)
p r.to_a.length
p r.map(&:day)
p r.first(2).map(&:day)
p r.include?(Date.new(2001, 1, 2))
p (Date.new(2001, 1, 1)...Date.new(2001, 1, 3)).to_a.length

# A user class needs only the two methods.
class Tick
  include Comparable
  attr_reader :n

  def initialize(n) = @n = n
  def succ = Tick.new(@n + 1)
  def <=>(o) = @n <=> o.n
  def inspect = "T#{@n}"
end

p (Tick.new(1)..Tick.new(4)).to_a
p (Tick.new(1)...Tick.new(4)).to_a
p (Tick.new(1)..Tick.new(4)).map(&:n)
p (Tick.new(3)..Tick.new(1)).to_a

# A value with NO `succ` still cannot be walked, and ruby names the class.
begin
  (1.0..2.0).each { }
rescue TypeError => e
  puts e.message
end
begin
  (nil..3).each { }
rescue TypeError => e
  puts e.message
end

# The three built-in walks are untouched.
p (1..4).to_a
p ("a".."d").to_a
p (:a..:d).to_a
p (2**70..2**70 + 2).to_a.length

# `reverse_each` on an ENDLESS range refuses LAZILY: blockless it answers
# an Enumerator, and the raise comes when it is walked.
e = (1..).reverse_each
puts e.class
begin
  e.next
rescue TypeError => err
  puts err.message
end
