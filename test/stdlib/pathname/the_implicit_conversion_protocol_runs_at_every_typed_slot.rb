# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# One ring: `loop_a = ["x"]; loop_a << loop_a`, the self-referential array
# that makes `File.join` raise ArgumentError. `ZEO_RT_GCRINGS=1` names it --
# "Array[2] (held by Array[2], holds Array[2])" is a two-element array that
# holds itself, and the only such array in the file.
#@ gccheck: cycle leak: 1 objects (Array x1)
require "tmpdir"
ZTMP = Dir.mktmpdir

require "pathname"
require "stringio"

dir = File.join(ZTMP, "spinel_implicit_conversion_protocol")
if Dir.exist?(dir)
  Dir.children(dir).each { |e| File.delete("#{dir}/#{e}") }
  Dir.rmdir(dir)
end
Dir.mkdir(dir)
file = "#{dir}/a.txt"
File.write(file, "abcdef\n")

class Conf
  attr_accessor :path

  def to_path
    @path
  end
end

class PathLike
  def initialize(path)
    @path = path
  end

  def to_path
    @path
  end
end

class StrLike
  def initialize(path)
    @path = path
  end

  def to_str
    @path
  end
end

class Both
  def initialize(path)
    @path = path
  end

  def to_path
    @path
  end

  def to_str
    "/nonexistent/#{@path}"
  end
end

pl = PathLike.new(file)
p File.exist?(pl)
p File.file?(pl)
p File.read(pl)
p File.basename(pl)
p File.dirname(pl) == dir
p File.extname(pl)
p File.expand_path(PathLike.new("x"), dir) == "#{dir}/x"
p File.size(pl)
p File.readlines(pl)
p File.readlines(pl, chomp: true)
p IO.read(pl)
p IO.readlines(pl)
lines = []
File.foreach(pl) { |l| lines << l }
p lines
p File.open(pl) { |f| f.read(3) }
p File.open(pl, "r") { |f| f.gets }
p File.join("a", PathLike.new("b"), "c")
p File.stat(pl).size
p File.exist?(StrLike.new(file))
p File.read(StrLike.new(file))
p File.exist?(Both.new(file))
conf = Conf.new
conf.path = file
p File.exist?(conf)
conf.path = nil
begin
  File.exist?(conf)
rescue TypeError => e
  p [e.class, e.message]
end
p File.atime(pl).class
p File.owned?(pl)
p File.realdirpath(PathLike.new("a.txt"), dl = PathLike.new(dir)) == File.realpath(file)
p File.expand_path("a.txt", PathLike.new(dir)) == file
p File.absolute_path("a.txt", PathLike.new(dir)) == file
p File.utime(Time.at(0), Time.at(0), pl)
p File.join(["a", PathLike.new("b")], "c")

p Dir.exist?(dl)
p Dir.entries(dl).sort
p Dir.children(dl)
p Dir.glob(PathLike.new("#{dir}/*.txt")).map { |f| File.basename(f) }
d = Dir.open(dl)
p d.path == dir
d.close
p Dir.empty?(dl)
sub = Pathname.new("#{dir}/sub")
p Dir.mkdir(sub)
p Dir.exist?(sub)
p Dir.rmdir(sub)

pn = Pathname.new(file)
p File.exist?(pn)
p File.basename(pn)
p File.join(Pathname.new("a"), "b")
p IO.read(pn)

def size_of(path)
  File.size(path)
end
p size_of(file)
p size_of(pl)
p size_of(pn)
p size_of(StrLike.new(file))
p size_of(Both.new(file))
p IO.sysopen(pl).class

copy = "#{dir}/b.txt"
p IO.copy_stream(pl, PathLike.new(copy))
p File.read(copy)
io = File.open("/dev/null", "r")
p io.reopen(PathLike.new(copy), "r").read
io.close
moved = "#{dir}/c.txt"
p File.rename(PathLike.new(copy), PathLike.new(moved))
p File.exist?(moved)
link = "#{dir}/link"
p File.symlink(PathLike.new(moved), PathLike.new(link))
p File.readlink(PathLike.new(link)) == moved
p File.delete(PathLike.new(link), PathLike.new(moved))
p File.exist?(moved)

class ReadMode
  def to_int
    File::RDONLY
  end
end

class ModeName
  def to_str
    "r"
  end
end
p File.open(pl, ReadMode.new) { |f| f.read(2) }
p File.open(pl, ModeName.new) { |f| f.read(2) }
p File.open(pl, mode: ReadMode.new) { |f| f.read(2) }
class CreateMode
  def to_int
    File::WRONLY | File::CREAT
  end
end
made = "#{dir}/made.txt"
File.open(made, CreateMode.new, 0600) { |f| f.write("m") }
p format("%o", File.stat(made).mode & 0777)
File.delete(made)
File.open(made, "w", 0600) { |f| f.write("m") }
p format("%o", File.stat(made).mode & 0777)
File.delete(made)
File.open(made, "w", perm: 0600) { |f| f.write("m") }
p format("%o", File.stat(made).mode & 0777)
File.delete(made)
File.open(made, mode: "w", perm: 0640) { |f| f.write("m") }
p format("%o", File.stat(made).mode & 0777)
File.delete(made)
File.open(made, File::WRONLY | File::CREAT, perm: 0600) { |f| p f.path == made }
p format("%o", File.stat(made).mode & 0777)
File.delete(made)
def maybe_perm(x)
  x ? 0600 : nil
end
File.open(made, "w", maybe_perm(false)) { |f| f.write("m") }
p format("%o", File.stat(made).mode & 0777)
File.open(made, "w", nil) { |f| p f.close_on_exec? }
begin
  File.open(made, "wx", 0600) { |f| f.write("!") }
rescue SystemCallError => e
  p e.class
end
p File.read(made)
File.delete(made)
begin
  File.open(made, "+", 0600)
rescue ArgumentError => e
  p e.message
end
begin
  File.open(made, "rx", 0600)
rescue ArgumentError => e
  p e.message
end
begin
  File.open(made, "ax")
rescue ArgumentError => e
  p e.message
end
File.open(made, "wx+", 0600) { |f| f.write("zz"); f.rewind; p f.read }
File.delete(made)

class Counted
  def initialize(path)
    @path = path
    @calls = 0
  end

  attr_reader :calls

  def to_path
    @calls += 1
    @path
  end
end
counted = Counted.new(file)
p [File.file?(counted), counted.calls]

class Zone
  def to_str
    "+01:00"
  end
end
p Time.at(0).getlocal(Zone.new).utc_offset
p Time.at(0).localtime(Zone.new).utc_offset
t = Time.at(0)
t.localtime(Zone.new)
p t.utc_offset

class Payload
  def to_s
    "payload"
  end
end
sio = StringIO.new
p sio.write(Payload.new)
p (sio << Payload.new).equal?(sio)
p sio.write(42)
p (sio << :sym).equal?(sio)
p sio.print(Payload.new, 1)
p sio.puts(Payload.new)
p sio.puts
result = sio.puts("assigned")
p result
class Nul
  def to_s
    "a\0b"
  end
end
p sio.print(Nul.new)
p sio.print(:sym, [1, 2])
class Plain
end
sio.puts(Plain.new)
p sio.string.sub(/0x[0-9a-f]+/, "0xADDR")
mixed = StringIO.new
[1, "a", :b].each { |x| mixed.puts(x) }
p mixed.string
File.open(file, "r+") { |f| p [f.pwrite(Payload.new, 1), File.read(file)] }
File.open(file, "w") { |f| p f.write_nonblock(Payload.new) }
p File.read(file)
p File.write(pl, Payload.new)
p File.write(pl, 42, mode: "a")
p File.write(pl, :sym, 2)
p File.read(file)
def payload(nul)
  nul ? "a\0b" : Payload.new
end
p File.write(pl, payload(true))
p File.size(file)
p File.write(pl, payload(false))

class Fresh
  def initialize(n)
    @n = n
  end
  def to_path
    "seg" + @n.to_s
  end
end
bad = 0
2000.times do |i|
  r = File.join(Fresh.new(i), Fresh.new(i + 1), Fresh.new(i + 2))
  bad += 1 unless r == "seg#{i}/seg#{i + 1}/seg#{i + 2}"
end
p bad
class Churn
  def initialize(n)
    @n = n
  end
  def to_path
    2000.times { "z" * 40 }
    "#{DIR}/churn#{@n}.txt"
  end
end
class Loud
  def to_s
    2000.times { "z" * 40 }
    "PAY"
  end
end
DIR = dir
p File.write(Churn.new(1), Loud.new)
File.rename(Churn.new(1), Churn.new(2))
p [File.exist?("#{dir}/churn1.txt"), File.exist?("#{dir}/churn2.txt")]
p File.expand_path(Churn.new(1), Churn.new(2)) == "#{dir}/churn2.txt#{dir}/churn1.txt"
File.delete("#{dir}/churn2.txt")
class Built
  def to_path
    b = ""
    50.times { b = b + "ab" }
    b
  end
end
bad = 0
2000.times { bad += 1 unless File.join(Built.new) == "ab" * 50 }
p bad
loop_a = ["x"]
loop_a << loop_a
begin
  File.join(loop_a)
rescue ArgumentError => e
  p e.message
end

class Noisy
  def initialize(n)
    @n = n
  end
  def to_s
    $stdout.print("[c#{@n}]")
    "<#{@n}>"
  end
end
$stdout.write(Noisy.new(5), Noisy.new(6))
puts
def named
  puts "named"
  "#{DIR}/named.txt"
end
File.write(named, Payload.new)
File.write("#{dir}/churn1.txt", "x")
File.rename(Churn.new(1), named)
p [File.exist?("#{dir}/churn1.txt"), File.read("#{dir}/named.txt")]
File.delete("#{dir}/named.txt")
class Wide
  def initialize(s)
    @s = s
  end
  def to_path
    ("y" * 3000).sub("y", @s)[0, 3]
  end
end
letters = ("a".."t").to_a
bad = 0
100.times do
  r = File.join(Wide.new("a"), Wide.new("b"), Wide.new("c"), Wide.new("d"), Wide.new("e"),
                Wide.new("f"), Wide.new("g"), Wide.new("h"), Wide.new("i"), Wide.new("j"),
                Wide.new("k"), Wide.new("l"), Wide.new("m"), Wide.new("n"), Wide.new("o"),
                Wide.new("p"), Wide.new("q"), Wide.new("r"), Wide.new("s"), Wide.new("t"))
  bad += 1 unless r == letters.map { |x| x + "yy" }.join("/")
end
p bad

def trace(label, value)
  puts label
  value
end
def trace_io(label, io)
  puts label
  io
end
p File.exist?(trace("arg", pl))
p trace_io("recv", sio).write(trace("arg", Payload.new))
class Announced
  def initialize(path)
    @path = path
  end
  def to_path
    puts "convert"
    @path
  end
end
def sideff(n)
  puts "sibling"
  n
end
announced = Announced.new(file)
p File.read(announced, sideff(4))
class Offset
  def to_int
    puts "convert"
    2
  end
end
off = Offset.new
p "abcdef"[off, sideff(3)]
class Spoken
  def to_s
    puts "convert"
    "payl"
  end
end
spoken = Spoken.new
File.open(file, "w") { |f| p f.write(spoken, sideff("Z")) }
p File.read(file)

File.delete(file)
Dir.rmdir(dir)
p Dir.exist?(dl)
__END__
true
true
"abcdef\n"
"a.txt"
true
".txt"
true
7
["abcdef\n"]
["abcdef"]
"abcdef\n"
["abcdef\n"]
["abcdef\n"]
"abc"
"abcdef\n"
"a/b/c"
7
true
"abcdef\n"
true
true
[TypeError, "no implicit conversion of nil into String"]
Time
true
true
true
true
1
"a/b/c"
true
[".", "..", "a.txt"]
["a.txt"]
["a.txt"]
true
false
0
true
0
true
"a.txt"
"a/b"
"abcdef\n"
7
7
7
7
7
Integer
7
"abcdef\n"
"abcdef\n"
0
true
0
true
2
false
"ab"
"ab"
"ab"
"600"
"600"
"600"
"640"
true
"600"
"644"
true
Errno::EEXIST
""
"invalid access mode +"
"invalid access mode rx"
"invalid access mode ax"
"zz"
[true, 1]
3600
3600
3600
7
true
2
true
nil
nil
nil
nil
nil
nil
"payloadpayload42sympayload1payload\n\nassigned\na\u0000bsym[1, 2]#<Plain:0xADDR>\n"
"1\na\nb\n"
[7, "apayload"]
7
"payload"
7
2
3
"pasymad42"
3
3
7
0
3
[false, true]
false
0
"recursive array"
[c5]<5>[c6]<6>
named
named
[false, "x"]
0
arg
true
recv
arg
7
sibling
convert
"payl"
sibling
convert
"cde"
sibling
convert
5
"paylZ"
false
