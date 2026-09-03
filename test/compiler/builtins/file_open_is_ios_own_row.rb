# `open`, `new`, `for_fd` and `sysopen` are IO's rows, and File INHERITS them.
#
# CRuby defines all four on IO's singleton class alone (`io.c`'s `Init_IO`; the
# `rb_define_singleton_method(rb_cFile, "open", ...)` beside them sits inside an
# `#if 0` that exists only to make RDoc document `File::open`). The RECEIVER
# then decides what the first argument means, because `rb_io_s_open` calls
# `klass.new`, which reaches `rb_file_initialize` for a File and
# `rb_io_initialize` for an IO.
#
# That one indirection decides four separate answers, and zeo -- which declared
# `File.open` in File's own block -- got each of them wrong: `#owner` named
# File, `File.for_fd` and `File.sysopen` did not exist at all, a subclass
# receiver lost its class, and one method could not report `1..3` to File and
# `1..2` to IO.
require "tmpdir"
require "fileutils"

DIR = Dir.mktmpdir("zeo_io_rows")
SRC = File.join(DIR, "src.txt")
File.binwrite(SRC, "hello\n")

def row(name)
  puts "#{name}\t#{yield.inspect}"
rescue StandardError => e
  puts "#{name}\t#{e.class}: #{e.message}"
end

# --- where the rows live -----------------------------------------------------
%w[open new for_fd sysopen].each do |m|
  [IO, File].each { |k| row("#{k}.#{m} owner") { k.method(m).owner.to_s } }
end
row("File declares open")   { File.singleton_class.instance_methods(false).include?(:open) }
row("IO declares open")     { IO.singleton_class.instance_methods(false).include?(:open) }
row("File responds to open"){ File.respond_to?(:open) }
row("one method, not two")  { File.method(:open).owner == IO.method(:open).owner }

# --- the receiver decides the arity, because it decides the `initialize` -----
row("IO.new range")    { IO.new }
row("File.new range")  { File.new }
row("IO.open range")   { IO.open }
row("File.open range") { File.open }
row("IO#initialize owner")   { IO.instance_method(:initialize).owner.to_s }
row("File#initialize owner") { File.instance_method(:initialize).owner.to_s }

# --- ...and what the first argument means -----------------------------------
row("File.open takes a path") { File.open(SRC) { |f| [f.class.to_s, f.read(5)] } }
row("IO.new takes a path")    { IO.new(SRC) }
row("File.new takes an fd")   { fd = IO.sysopen(SRC); io = File.new(fd); r = [io.class.to_s, io.read(5)]; io.close; r }
# `rb_io_s_for_fd` calls `rb_io_initialize` BY NAME rather than dispatching, so
# a File receiver reads a descriptor here where `File.new` would still be
# asking File what its argument means.
row("File.for_fd takes an fd") { fd = IO.sysopen(SRC); io = File.for_fd(fd); r = [io.class.to_s, io.read(5)]; io.close; r }
row("File.sysopen answers an fd") { fd = File.sysopen(SRC); io = IO.for_fd(fd); r = io.read(5); io.close; r }

# --- a subclass receiver gets an instance of ITSELF --------------------------
class Journal < File; end
row("named File subclass") { Journal.open(SRC) { |f| f.class.to_s } }
row("named subclass new")  { f = Journal.new(SRC); r = f.class.to_s; f.close; r }
row("anonymous subclass")  { c = Class.new(File); c.open(SRC) { |f| f.class == c } }
class Wire < IO; end
row("named IO subclass")   { io = Wire.new(IO.sysopen(SRC)); r = io.class.to_s; io.close; r }
row("File.for_fd subclass"){ io = Journal.for_fd(IO.sysopen(SRC)); r = io.class.to_s; io.close; r }

# --- `new` never yields, so a block there is a mistake CRuby warns about ----
f = File.new(SRC) { |x| :never_yielded }
p [f.class.to_s, f.read(5)]
f.close

FileUtils.remove_entry(DIR)
puts "ran to the end"
__END__
IO.open owner	"#<Class:IO>"
File.open owner	"#<Class:IO>"
IO.new owner	"#<Class:IO>"
File.new owner	"#<Class:IO>"
IO.for_fd owner	"#<Class:IO>"
File.for_fd owner	"#<Class:IO>"
IO.sysopen owner	"#<Class:IO>"
File.sysopen owner	"#<Class:IO>"
File declares open	false
IO declares open	true
File responds to open	true
one method, not two	true
IO.new range	ArgumentError: wrong number of arguments (given 0, expected 1..2)
File.new range	ArgumentError: wrong number of arguments (given 0, expected 1..3)
IO.open range	ArgumentError: wrong number of arguments (given 0, expected 1..2)
File.open range	ArgumentError: wrong number of arguments (given 0, expected 1..3)
IO#initialize owner	"IO"
File#initialize owner	"File"
File.open takes a path	["File", "hello"]
IO.new takes a path	TypeError: no implicit conversion of String into Integer
File.new takes an fd	["File", "hello"]
File.for_fd takes an fd	["File", "hello"]
File.sysopen answers an fd	"hello"
named File subclass	"Journal"
named subclass new	"Journal"
anonymous subclass	true
named IO subclass	"Wire"
File.for_fd subclass	"Journal"
["File", "hello"]
ran to the end
#@ stderr
compiler/builtins/file_open_is_ios_own_row.rb:65: warning: File::new() does not take block; use File::open() instead
