base = "/tmp/zeo_w1i_example"

# Start clean without external requires: remove any leftover entries.
if Dir.exist?(base)
  Dir.each_child(base) do |name|
    path = File.join(base, name)
    File.directory?(path) ? Dir.rmdir(path) : File.delete(path)
  end
  Dir.rmdir(base)
end
Dir.mkdir(base)

File.write(File.join(base, "a.txt"), "one\ntwo\nthree\n")
File.write(File.join(base, "b.log"), "x")
Dir.mkdir(File.join(base, "sub"))

# File.ftype distinguishes file vs directory
p File.ftype(File.join(base, "a.txt"))
p File.ftype(File.join(base, "sub"))

# File.foreach yields lines; chomp: strips terminators
File.foreach(File.join(base, "a.txt")) { |line| print "L:", line }
p File.foreach(File.join(base, "a.txt"), chomp: true).to_a

# Dir.foreach yields entries (including . and ..); block form returns nil
entries = []
result = Dir.foreach(base) { |e| entries << e }
p entries.sort
p result

# Dir.foreach without a block is an Enumerator
p Dir.foreach(base).to_a.sort
__END__
"file"
"directory"
L:one
L:two
L:three
["one", "two", "three"]
[".", "..", "a.txt", "b.log", "sub"]
nil
[".", "..", "a.txt", "b.log", "sub"]
