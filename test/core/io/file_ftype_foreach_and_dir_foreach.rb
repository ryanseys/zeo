require "tmpdir"
Dir.mktmpdir do |dir|
  File.write(File.join(dir, "a.txt"), "one\ntwo\nthree\n")
  Dir.mkdir(File.join(dir, "sub"))
  p File.ftype(File.join(dir, "a.txt"))
  p File.ftype(File.join(dir, "sub"))
  File.foreach(File.join(dir, "a.txt")) { |line| print "L:", line }
  p File.foreach(File.join(dir, "a.txt"), chomp: true).to_a
  # Enumerator form gives the entries; block form returns nil.
  p Dir.foreach(dir).to_a.sort
  p(Dir.foreach(dir) { |e| })
end
__END__
"file"
"directory"
L:one
L:two
L:three
["one", "two", "three"]
[".", "..", "a.txt", "sub"]
nil
