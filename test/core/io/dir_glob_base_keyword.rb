require "tmpdir"

Dir.mktmpdir do |dir|
  Dir.mkdir(File.join(dir, "sub"))
  File.write(File.join(dir, "a.txt"), "x")
  File.write(File.join(dir, "sub", "b.txt"), "y")
  p Dir.glob("**/*.txt", base: dir).sort
  p Dir.glob("*.txt", base: dir).sort
  p Dir.glob("**/*.txt", base: dir).map { |f| File.file?(File.join(dir, f)) }
end
__END__
["a.txt", "sub/b.txt"]
["a.txt"]
[true, true]
