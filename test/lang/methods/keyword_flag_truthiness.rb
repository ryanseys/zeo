require "tmpdir"
ZTMP = Dir.mktmpdir

p "ab\n".each_line(chomp: 1).to_a
p "ab\n".each_line(chomp: nil).to_a
p "ab\ncd\n".lines(chomp: 1)
p "ab\ncd\n".lines(chomp: false)
p "ab\ncd\n".lines("\n", chomp: :yes)
p "ab\ncd\n".lines("\n", chomp: nil)
f = false
t = "truthy"
p "ab\n".lines(chomp: f)
p "ab\n".lines(chomp: t)
p "ab\n".each_line(chomp: t).to_a
one = File.join(ZTMP, "zeo_kwflag_#{Process.pid}_1.txt")
two = File.join(ZTMP, "zeo_kwflag_#{Process.pid}_2.txt")
File.write(one, "a\nb\nc\n")
File.open(one) do |fh|
  p fh.gets(chomp: "yes")
  p fh.readline(chomp: nil)
  p fh.gets(chomp: t)
end
File.delete(one)
File.write(two, "a\nb\n")
p File.readlines(two, chomp: 1)
p File.readlines(two, chomp: f)
p File.readlines(two, chomp: t)
File.open(two) { |fh| p fh.readlines(chomp: :y) }
File.open(two) { |fh| p fh.readlines(chomp: t) }
File.delete(two)
__END__
["ab"]
["ab\n"]
["ab", "cd"]
["ab\n", "cd\n"]
["ab", "cd"]
["ab\n", "cd\n"]
["ab\n"]
["ab"]
["ab"]
"a"
"b\n"
"c"
["a", "b"]
["a\n", "b\n"]
["a", "b"]
["a", "b"]
["a", "b"]
