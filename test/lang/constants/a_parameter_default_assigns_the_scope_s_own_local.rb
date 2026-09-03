# `def discard(key = (no_arg = true))` is ruby's idiom for "was an argument
# passed?" -- the default expression assigns a local, and the body reads it.
# That local belongs to the scope being DEFINED: the default runs in the
# callee's own frame, only when the argument is omitted.
#
# Reading only the body saw the read and no assignment, so an inner block
# spelling it this way looked like it captured its enclosing block's own
# local -- the one nesting shape codegen refuses. roda's flash plugin and
# pantheios' thread naming both write it.
class Flash
  def initialize = @seen = []

  def discard(key = (no_arg = true))
    no_arg ? :cleared_all : [:cleared, key]
  end
end

p Flash.new.discard
p Flash.new.discard(:notice)

# The same default inside a `define_method` written in a block -- the nesting
# that made this a rejection rather than a wrong answer.
class Named; end
Named.instance_eval do
  define_method :thread_name do |name = (name_not_given = true)|
    if name_not_given
      "unnamed"
    else
      "named-#{name}"
    end
  end
end

p Named.new.thread_name
p Named.new.thread_name("worker")

# A keyword default assigns the same way, and a default that reads an EARLIER
# parameter still sees it.
def sized(width, height = (square = true; width), unit: (bare = true; "px"))
  [width, height, unit, square, bare]
end

p sized(3)
p sized(3, 4, unit: "em")

# The enclosing scope keeps its OWN name of the same spelling: the default's
# assignment never reaches out.
square = :outer
p sized(2)
p square
puts "still running"
__END__
:cleared_all
[:cleared, :notice]
"unnamed"
"named-worker"
[3, 3, "px", true, true]
[3, 4, "em", nil, nil]
[2, 2, "px", true, true]
:outer
still running
