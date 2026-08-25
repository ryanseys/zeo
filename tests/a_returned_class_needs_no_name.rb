# Every line here reaches a builtin class this program NAMES NOWHERE.
#
# The compiler decides which builtin method tables the binary carries
# (`analyze::class_reach`), and a class it drops that the program does reach
# aborts at the first dispatch. Constants are the easy channel; this is the
# hard one -- a row RETURNS an instance of a class no scan of the source can
# see. Keep it name-free: writing the class name here would test nothing.

def frame = caller_locations(1, 1)
p frame.first.class

p binding.class
p method(:p).class
p 1.quo(3)
p 1.quo(3).class
p "x".encoding.class
p [1, 2].lazy.first(1)
p [1, 2].lazy.class
p [1, 2, 3].chunk_while { |a, b| b == a + 1 }.to_a
p [1, 2].each_slice(1).class
p (1..10).step(3).class
p 2.to_c.class

srand(7)
p rand(2).class

module Louder
  refine String do
    def shout = upcase
  end
end
using Louder
p "hi".shout

p __dir__.class
