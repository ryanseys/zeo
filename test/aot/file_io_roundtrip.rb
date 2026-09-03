# File and directory calls in the binary: write, read back, stat and remove.
require "tmpdir"
Dir.mktmpdir("zeo-aot") do |dir|
  path = File.join(dir, "note.txt")
  File.write(path, "line one\nline two\n")
  puts File.size(path)
  puts File.readlines(path).map(&:chomp).inspect
  File.open(path, "a") { |f| f.puts "line three" }
  puts File.read(path).lines.length
  puts File.basename(path), File.extname(path)
  puts Dir.children(dir).inspect
end
__END__
18
["line one", "line two"]
3
note.txt
.txt
["note.txt"]
