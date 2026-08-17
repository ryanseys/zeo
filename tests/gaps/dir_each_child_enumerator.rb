require "tmpdir"

Dir.mktmpdir do |d|
  File.write(File.join(d, "a.txt"), "x")
  p Dir.each_child(d).to_a
  p Dir.foreach(d).to_a.sort
end
