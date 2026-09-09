# It answers the byte count, and the file holds them.
# (spinel issue #3217)
require "tmpdir"
ZTMP = Dir.mktmpdir

require 'stringio'
io = StringIO.new("hello world")
File.open(File.join(ZTMP, "spinel_issue_3217_out.txt"), "w") do |f|
  n = IO.copy_stream(io, f)
  p n
end
puts File.read(File.join(ZTMP, "spinel_issue_3217_out.txt"))
File.write(File.join(ZTMP, "spinel_issue_3217_in.txt"), "abc123")
sink = StringIO.new
File.open(File.join(ZTMP, "spinel_issue_3217_in.txt")) do |f|
  IO.copy_stream(f, sink)
end
puts sink.string
__END__
11
hello world
abc123
