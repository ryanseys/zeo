# `File.open`: the block form closes on every exit path and answers the
# block's value. A LENGTHED read at EOF is nil where a whole-rest read is
# `""` -- the asymmetry a `while chunk = f.read(n)` loop relies on.

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_open_#{Process.pid}")
Dir.mkdir(dir)
begin
  path = File.join(dir, "counted.txt")
  File.open(path, "w") { |f| f.print "ABCDEFGHIJ" }
  p File.read(path)

  File.open(path, "r") do |f|
    p f.read(5)
    p f.read(5)
    p f.read(5)
    f.rewind
    p f.read(2)
    p f.tell
    f.seek(0)
    p f.read
    p f.eof?
  end

  p File.open(path, "r") { |f| f.read(3) }

  h = File.open(path, "r")
  p h.closed?
  h.close
  p h.closed?
  begin
    h.read
  rescue IOError => e
    puts "IOError: #{e.message}"
  end

  File.delete(path)
ensure
  Dir.rmdir(dir)
end
__END__
"ABCDEFGHIJ"
"ABCDE"
"FGHIJ"
nil
"AB"
2
"ABCDEFGHIJ"
true
"ABC"
false
true
IOError: closed stream
