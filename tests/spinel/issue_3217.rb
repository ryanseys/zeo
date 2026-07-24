require 'stringio'
io = StringIO.new("hello world")
File.open("/tmp/spinel_issue_3217_out.txt", "w") do |f|
  n = IO.copy_stream(io, f)
  p n
end
puts File.read("/tmp/spinel_issue_3217_out.txt")
File.write("/tmp/spinel_issue_3217_in.txt", "abc123")
sink = StringIO.new
File.open("/tmp/spinel_issue_3217_in.txt") do |f|
  IO.copy_stream(f, sink)
end
puts sink.string
