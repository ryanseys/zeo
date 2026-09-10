require "tmpdir"
ZTMP = Dir.mktmpdir

File.write(File.join(ZTMP, "tfg.tmp"), "line1\nline2\n")
f = File.open(File.join(ZTMP, "tfg.tmp"))
t = Thread.new do
  a = f.gets
  b = f.gets
  [a, b]
end
p t.value
f.close
File.delete(File.join(ZTMP, "tfg.tmp"))
__END__
["line1\n", "line2\n"]
