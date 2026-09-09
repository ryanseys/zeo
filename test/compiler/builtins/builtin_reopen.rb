# Reopening a builtin class at the top level: fresh methods, overrides of
# native methods, class-level state and protocol overrides all attach to the
# real class and win everywhere ruby's would.

class String
  def blank?
    length == 0
  end

  def shout
    exclaim + "?"
  end

  def exclaim
    self + "!"
  end
end

class Integer
  def repeat(sep = "-")
    out = ""
    i = 0
    while i < self
      out = out + yield(i).to_s
      out = out + sep if i < self - 1
      i = i + 1
    end
    out
  end
end

class Array
  @@made = 0
  LIMIT = 3

  def self.tally_up
    @@made = @@made + 1
    @@made
  end

  def under_limit?
    length < LIMIT
  end
end

class Range
  def span
    self.last - self.first
  end
end

class NilClass
  def describe
    "nothing"
  end
end

puts "".blank?
puts "  hi".blank?
puts "hey".shout

puts 3.repeat { |i| i * 2 }
puts 2.repeat("+") { |i| i + 1 }

puts Array.tally_up
puts Array.tally_up
puts [1, 2].under_limit?
puts [1, 2, 3, 4].under_limit?
puts Array::LIMIT

puts (3..9).span
puts nil.describe

# A user override of an EXISTING native method wins at every call site --
# static and dynamic -- while `length` (a separate method, not an alias)
# stays native. (Overriding a name no earlier statement calls, so the
# compile-time class model and Ruby's sequential execution agree.)
class Hash
  def size
    42
  end
end

h = { a: 1, b: 2 }
puts h.size
puts [h].first.size
puts h.length

# An unknown method on a builtin still raises real Ruby's NoMethodError.
begin
  "hey".nope
rescue NoMethodError => e
  puts e.message
end
__END__
true
false
hey!?
0-2-4
1+2
1
2
true
false
3
6
nothing
42
42
2
undefined method 'nope' for an instance of String
