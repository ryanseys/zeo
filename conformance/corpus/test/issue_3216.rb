require 'stringio'

def http_resp(dst, src)
  src.rewind
  IO.copy_stream(src, dst)
end

src = StringIO.new("hello world")
dst = StringIO.new
n = http_resp(dst, src)
p n
p dst.string

# partial copy respects the source position
src2 = StringIO.new("abcdef")
src2.read(2)
dst2 = StringIO.new
IO.copy_stream(src2, dst2)
p dst2.string
