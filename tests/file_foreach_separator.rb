require "tmpdir"

Dir.mktmpdir do |d|
  f = File.join(d, "o.txt")
  File.write(f, "l1\nl2\n")
  p File.foreach(f, "1").first
  p File.foreach(f).first
end
