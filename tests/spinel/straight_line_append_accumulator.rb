# An accumulator appended to by straight-line statements takes the growable
# handle the same way one appended to inside a loop does -- a template
# compiler emits one `io << chunk` per literal chunk with no loop, and a
# whole-buffer copy per append makes building the page quadratic (#3480).
# The cases here are the ones the promotion has to keep answering the same:
# a read before the appending is finished (which must NOT promote), aliasing,
# freezing, passing the accumulator on, and interpolation.
def render
  io = String.new
  io << "<p>a</p>"
  io << "<p>b</p>"
  io << "<p>c</p>"
  io
end

def read_between
  io = String.new
  io << "one"
  mid = io.length          # a read before the appending is done
  io << "two"
  "#{io}/#{mid}"
end

def aliased
  io = String.new
  io << "x"
  io << "y"
  other = io               # alias after the appends
  io << "z"                # mutation after aliasing
  [io, other, io.equal?(other)]
end

def to_helper(s)
  s.length
end

def passed
  io = String.new
  io << "abc"
  io << "de"
  to_helper(io) + io.length
end

def frozen_case
  io = String.new
  io << "p"
  io << "q"
  f = io.dup.freeze
  begin
    f << "r"
    "no raise"
  rescue FrozenError, RuntimeError
    "raised"
  end
end

def binary_case
  io = String.new
  io << "a b"
  io << "c d"
  [io.bytesize, io.bytes.length]
end

def interpolated(n)
  io = String.new
  io << "n=#{n};"
  io << "sq=#{n * n};"
  io
end

p render
p render.length
p read_between
p aliased
p passed
p frozen_case
p binary_case
p interpolated(7)
p render == "<p>a</p><p>b</p><p>c</p>"
p render.frozen?
