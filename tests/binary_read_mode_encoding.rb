require "tmpdir"

Dir.mktmpdir do |d|
  f = File.join(d, "m.txt")
  File.write(f, "abc")
  p File.open(f, "rb") { |io| io.read.encoding.name }
  p File.open(f, "r", encoding: "BINARY") { |io| io.read.encoding.name }
  p File.binread(f).encoding.name
end
