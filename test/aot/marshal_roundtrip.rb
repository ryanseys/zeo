# Marshal writes and reads a graph of core objects inside the binary.
Point = Struct.new(:x, :y)
data = { list: [1, 2.5, "three", :four], point: Point.new(6, 7), set: (1..3).to_a }
bytes = Marshal.dump(data)
back = Marshal.load(bytes)
p back[:list]
p back[:point].to_a
p back == data
puts bytes.encoding
__END__
[1, 2.5, "three", :four]
[6, 7]
true
ASCII-8BIT
