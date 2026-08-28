require "socket"

class Wrapper
  def initialize(io)
    @io = io
  end
  def to_io
    @io
  end
end

r, w = IO.pipe
wr = Wrapper.new(r)

p IO.select([wr], nil, nil, 0).nil?

w.write("hi")
w.flush
got = IO.select([wr], nil, nil, 1)
p got.nil?
p got[0].length
p got[0][0].equal?(wr)
p r.read_nonblock(2)

ww = Wrapper.new(w)
ready = IO.select(nil, [ww], nil, 1)
p ready.nil?
p ready[1][0].equal?(ww)

w.write("x")
w.flush
mixed = IO.select([wr, r], nil, nil, 1)
p mixed[0].length

r.close
w.close

class NotAnIo
end
begin
  IO.select([NotAnIo.new], nil, nil, 0)
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
