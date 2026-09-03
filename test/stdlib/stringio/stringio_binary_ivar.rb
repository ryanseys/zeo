require "stringio"

class Bits
  def initialize
    @io = StringIO.new
  end

  def add(byte)
    @io.write([byte].pack("C"))
  end

  def head(s)
    @io << s
  end

  def out
    @io.string
  end
end

b = Bits.new
b.head("hdr\n")
[0, 8, 0, 255, 0].each { |v| b.add(v) }
b.head("!")
p b.out.bytes
p b.out.bytesize

io = StringIO.new
io << "a\0b"
io.write("\0")
io.print("x\0y")
p io.string.bytes
p io.size
__END__
[104, 100, 114, 10, 0, 8, 0, 255, 0, 33]
10
[97, 0, 98, 0, 120, 0, 121]
7
