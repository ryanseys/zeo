# `Pathname` is reachable with NO require: ruby 4.0 loads `pathname.so` before
# the first line. This covers the whole surface -- the path ALGEBRA, which is
# pure string work and never touches disk, and the delegations, which must
# answer exactly what the same `File`/`Dir` call answers.

require "pathname"   # a no-op here and in ruby 4.0: the class is already there
require "tmpdir"

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e
  puts "#{label}: #{e.class}: #{e.message}"
end

show("ancestors") { Pathname.ancestors }
show("own singleton") { Pathname.singleton_methods(false).sort }
show("own instance count") { Pathname.instance_methods(false).size }
show("constants") { Pathname.constants.sort }
show("VERSION") { Pathname::VERSION }
show("SEPARATOR_PAT") { Pathname::SEPARATOR_PAT }
# `#path` is PROTECTED in ruby; see tests/gaps/builtin_protected_rows.rb.
show("Kernel#Pathname is private") { Kernel.private_instance_methods(false).include?(:Pathname) }

# Construction.
show("new") { Pathname.new("/a/b").to_s }
show("new from Pathname") { Pathname.new(Pathname.new("a")).to_s }
show("new frozen arg") { Pathname.new("a".freeze).to_s }
show("new null byte") { Pathname.new("a\0b") }
show("new Integer") { Pathname.new(1) }
show("new no args") { Pathname.new }
show("Kernel#Pathname") { Pathname("/x").class }

# Identity and ordering. `Pathname` does NOT include Comparable, however much
# its `#<=>` suggests otherwise.
show("to_s") { Pathname.new("a").to_s }
show("to_s is a fresh String") { p1 = Pathname.new("a"); !p1.to_s.equal?(p1.to_s) }
show("to_s is unfrozen") { Pathname.new("a").to_s.frozen? }
show("mutating to_s leaves self") { p1 = Pathname.new("a"); p1.to_s << "X"; p1.to_s }
show("inspect") { Pathname.new("/a/b").inspect }
show("== self") { Pathname.new("a") == Pathname.new("a") }
show("== String") { Pathname.new("a") == "a" }
show("===") { Pathname.new("a") === Pathname.new("a") }
show("eql?") { Pathname.new("a").eql?(Pathname.new("a")) }
show("hash agrees with ==") { Pathname.new("a").hash == Pathname.new("a").hash }
show("<=>") { Pathname.new("a") <=> Pathname.new("b") }
show("<=> String") { Pathname.new("a") <=> "a" }
show("no Comparable") { Pathname.new("a") < Pathname.new("b") }
show("freeze") { p1 = Pathname.new("a"); p1.freeze; p1.frozen? }
show("fresh is unfrozen") { Pathname.new("a").frozen? }

# `#+` resolves `.` and `..` against the LEFT operand's components, without
# ever asking the filesystem.
show("plus") { (Pathname.new("/a") + "b").to_s }
show("plus absolute") { (Pathname.new("/a") + "/b").to_s }
show("plus dot") { (Pathname.new("a") + ".").to_s }
show("plus dotdot") { (Pathname.new("a/b") + "..").to_s }
show("plus dotdot deep") { (Pathname.new("a/b/c") + "../../d").to_s }
show("plus empty left") { (Pathname.new("") + "b").to_s }
show("plus root") { (Pathname.new("/") + "a").to_s }
show("plus trailing sep") { (Pathname.new("a/") + "b").to_s }
show("plus both relative") { (Pathname.new("a/b") + "c/d").to_s }
show("div") { (Pathname.new("a") / "b").to_s }
show("plus Pathname") { (Pathname.new("a") + Pathname.new("b")).to_s }
show("join") { Pathname.new("/a").join("b", "c").to_s }
show("join none") { Pathname.new("/a").join.to_s }
show("join absolute wins") { Pathname.new("/a").join("b", "/c").to_s }
show("parent") { Pathname.new("/a/b").parent.to_s }
show("parent relative") { Pathname.new("a").parent.to_s }
show("parent root") { Pathname.new("/").parent.to_s }

show("cleanpath") { Pathname.new("a/../b/./c//d").cleanpath.to_s }
show("cleanpath conservative") { Pathname.new("a/../b/./c//d").cleanpath(true).to_s }
show("cleanpath dot") { Pathname.new(".").cleanpath.to_s }
show("cleanpath empty") { Pathname.new("").cleanpath.to_s }
show("cleanpath dotdot") { Pathname.new("..").cleanpath.to_s }
show("cleanpath above root") { Pathname.new("/../a").cleanpath.to_s }
show("cleanpath trailing") { Pathname.new("a/").cleanpath.to_s }
show("cleanpath root") { Pathname.new("/").cleanpath.to_s }

show("relative_path_from") { Pathname.new("/a/b/c").relative_path_from("/a/d").to_s }
show("relative_path_from same") { Pathname.new("/a/b").relative_path_from("/a/b").to_s }
show("relative_path_from below") { Pathname.new("a/b").relative_path_from("a").to_s }
show("relative_path_from above") { Pathname.new("a").relative_path_from("a/b/c").to_s }
show("relative_path_from mixed") { Pathname.new("a").relative_path_from("/b") }
show("relative_path_from dotdot base") { Pathname.new("a").relative_path_from("../b") }

show("each_filename") { Pathname.new("/a/b/c").each_filename.to_a }
show("each_filename dotdot") { Pathname.new("a/b/../c").each_filename.to_a }
show("ascend") { Pathname.new("/a/b/c").ascend.map(&:to_s) }
show("ascend relative") { Pathname.new("a/b").ascend.map(&:to_s) }
show("ascend block") { r = []; Pathname.new("a/b").ascend { |x| r << x.to_s }; r }
show("descend") { Pathname.new("/a/b/c").descend.map(&:to_s) }
show("descend relative") { Pathname.new("a/b").descend.map(&:to_s) }

show("basename") { Pathname.new("/a/b.rb").basename.to_s }
show("basename suffix") { Pathname.new("/a/b.rb").basename(".rb").to_s }
show("dirname") { Pathname.new("/a/b.rb").dirname.to_s }
show("extname") { Pathname.new("/a/b.rb").extname }
show("sub_ext") { Pathname.new("a/b.rb").sub_ext(".txt").to_s }
show("sub_ext none") { Pathname.new("a/b").sub_ext(".txt").to_s }
show("sub_ext empty") { Pathname.new("a/b.rb").sub_ext("").to_s }
show("split") { Pathname.new("/a/b").split.map(&:to_s) }
show("split root") { Pathname.new("/").split.map(&:to_s) }
show("sub") { Pathname.new("/a/b").sub("a", "z").to_s }
show("sub block") { Pathname.new("/a/b").sub(/a/) { |m| m.upcase }.to_s }
show("expand_path") { Pathname.new("b").expand_path("/a").to_s }
show("fnmatch") { Pathname.new("a/b.rb").fnmatch("*/*.rb") }
show("fnmatch?") { Pathname.new("a/b.rb").fnmatch?("*.rb") }

show("absolute?") { [Pathname.new("/a").absolute?, Pathname.new("a").absolute?] }
show("relative?") { [Pathname.new("/a").relative?, Pathname.new("a").relative?] }
show("root?") { [Pathname.new("/").root?, Pathname.new("/a").root?, Pathname.new("").root?] }

# The delegations, over a tree this program builds.
Dir.mktmpdir do |tmp|
  root = Pathname.new(tmp)
  file = root + "hello.txt"
  file.write("hello\nworld\n")
  sub = root + "sub"
  sub.mkpath
  (sub + "deep").mkpath
  (sub + "deep" + "leaf.txt").write("leaf")

  show("write returns count") { (root + "w.txt").write("abcd") }
  show("read") { file.read }
  show("read n") { file.read(5) }
  show("binread") { file.binread(5) }
  show("readlines") { file.readlines }
  show("each_line") { r = []; file.each_line { |l| r << l }; r }
  show("open block") { file.open { |f| f.read(5) } }
  show("size") { file.size }
  show("size?") { [file.size?, (root + "empty").tap { |p| p.write("") }.size?] }
  show("zero?") { (root + "empty").zero? }
  show("empty? file") { (root + "empty").empty? }
  show("empty? dir") { [(root + "sub" + "deep").empty?, sub.empty?] }
  show("exist?") { [file.exist?, (root + "nope").exist?] }
  show("file?") { [file.file?, sub.file?] }
  show("directory?") { [file.directory?, sub.directory?] }
  show("ftype") { [file.ftype, sub.ftype] }
  show("readable?") { file.readable? }
  show("writable?") { file.writable? }
  show("executable?") { file.executable? }
  show("symlink?") { file.symlink? }
  show("stat class") { file.stat.class }
  show("lstat class") { file.lstat.class }
  show("mtime class") { file.mtime.class }
  show("atime class") { file.atime.class }
  show("ctime class") { file.ctime.class }
  show("chmod") { file.chmod(0o600) }
  show("mode after chmod") { format("%o", file.stat.mode & 0o777) }
  show("truncate") { f = (root + "t.txt"); f.write("abcdef"); f.truncate(3); f.read }
  show("rename") do
    f = (root + "old.txt")
    f.write("x")
    f.rename((root + "new.txt").to_s)
    [(root + "old.txt").exist?, (root + "new.txt").read]
  end
  show("delete") { f = (root + "gone.txt"); f.write("x"); f.delete; f.exist? }
  show("make_symlink") do
    link = root + "link"
    link.make_symlink(file.to_s)
    [link.symlink?, link.readlink.to_s == file.to_s]
  end
  show("make_link") do
    link = root + "hard"
    link.make_link(file.to_s)
    [link.exist?, link.symlink?]
  end
  show("realpath class") { file.realpath.class }
  show("realdirpath class") { file.realdirpath.class }
  show("children classes") { root.children.map(&:class).uniq }
  show("children sorted") { sub.children.map { |c| c.basename.to_s }.sort }
  show("children bare") { sub.children(false).map(&:to_s).sort }
  show("each_child") { r = []; sub.each_child { |c| r << c.basename.to_s }; r.sort }
  show("entries sorted") { sub.entries.map(&:to_s).sort }
  show("each_entry") { r = []; sub.each_entry { |c| r << c.to_s }; r.sort }
  show("glob") { root.glob("*.txt").map { |p| p.basename.to_s }.sort }
  show("glob block") do
    r = []
    root.glob("*.txt") { |p| r << p.basename.to_s }
    r.sort
  end
  show("find") { root.find.map { |p| p.relative_path_from(root).to_s }.sort }
  show("mkdir") { d = (root + "fresh"); d.mkdir; d.directory? }
  show("mkdir twice") { (root + "fresh").mkdir rescue "#{$!.class}: #{$!.message[/\A[^@]*/].strip}" }
  show("rmdir") { d = (root + "fresh"); d.rmdir; d.exist? }
  show("mkpath deep") { d = (root + "x" + "y" + "z"); d.mkpath; d.directory? }
  show("rmtree") { d = (root + "x"); d.rmtree; d.exist? }
  show("opendir") { root.opendir { |d| d.class } }
  show("mountpoint? tmp") { root.mountpoint? }
  show("mountpoint? root") { Pathname.new("/").mountpoint? }
end

show("getwd class") { Pathname.getwd.class }
show("pwd agrees with Dir") { Pathname.pwd.to_s == Dir.pwd }
show("Pathname.glob class") { Pathname.glob("*").class }
__END__
ancestors: [Pathname, Object, Kernel, BasicObject]
own singleton: [:getwd, :glob, :mktmpdir, :pwd]
own instance count: 98
constants: [:SEPARATOR_PAT, :VERSION]
VERSION: "0.4.0"
SEPARATOR_PAT: /\//
Kernel#Pathname is private: true
new: "/a/b"
new from Pathname: "a"
new frozen arg: "a"
new null byte: ArgumentError: path name contains null byte
new Integer: TypeError: Pathname.new requires a String, #to_path or #to_str
new no args: ArgumentError: wrong number of arguments (given 0, expected 1)
Kernel#Pathname: Pathname
to_s: "a"
to_s is a fresh String: true
to_s is unfrozen: false
mutating to_s leaves self: "a"
inspect: "#<Pathname:/a/b>"
== self: true
== String: false
===: true
eql?: true
hash agrees with ==: true
<=>: -1
<=> String: nil
no Comparable: NoMethodError: undefined method '<' for an instance of Pathname
freeze: true
fresh is unfrozen: false
plus: "/a/b"
plus absolute: "/b"
plus dot: "a"
plus dotdot: "a"
plus dotdot deep: "a/d"
plus empty left: "b"
plus root: "/a"
plus trailing sep: "a/b"
plus both relative: "a/b/c/d"
div: "a/b"
plus Pathname: "a/b"
join: "/a/b/c"
join none: "/a"
join absolute wins: "/c"
parent: "/a"
parent relative: "."
parent root: "/"
cleanpath: "b/c/d"
cleanpath conservative: "a/../b/c/d"
cleanpath dot: "."
cleanpath empty: "."
cleanpath dotdot: ".."
cleanpath above root: "/a"
cleanpath trailing: "a"
cleanpath root: "/"
relative_path_from: "../b/c"
relative_path_from same: "."
relative_path_from below: "b"
relative_path_from above: "../.."
relative_path_from mixed: ArgumentError: different prefix: "" and "/b"
relative_path_from dotdot base: ArgumentError: base_directory has ..: "../b"
each_filename: ["a", "b", "c"]
each_filename dotdot: ["a", "b", "..", "c"]
ascend: ["/a/b/c", "/a/b", "/a", "/"]
ascend relative: ["a/b", "a"]
ascend block: ["a/b", "a"]
descend: ["/", "/a", "/a/b", "/a/b/c"]
descend relative: ["a", "a/b"]
basename: "b.rb"
basename suffix: "b"
dirname: "/a"
extname: ".rb"
sub_ext: "a/b.txt"
sub_ext none: "a/b.txt"
sub_ext empty: "a/b"
split: ["/a", "b"]
split root: ["/", "/"]
sub: "/z/b"
sub block: "/A/b"
expand_path: "/a/b"
fnmatch: true
fnmatch?: true
absolute?: [true, false]
relative?: [false, true]
root?: [true, false, false]
write returns count: 4
read: "hello\nworld\n"
read n: "hello"
binread: "hello"
readlines: ["hello\n", "world\n"]
each_line: ["hello\n", "world\n"]
open block: "hello"
size: 12
size?: [12, nil]
zero?: true
empty? file: true
empty? dir: [false, false]
exist?: [true, false]
file?: [true, false]
directory?: [false, true]
ftype: ["file", "directory"]
readable?: true
writable?: true
executable?: false
symlink?: false
stat class: File::Stat
lstat class: File::Stat
mtime class: Time
atime class: Time
ctime class: Time
chmod: 1
mode after chmod: "600"
truncate: "abc"
rename: [false, "x"]
delete: false
make_symlink: [true, true]
make_link: [true, false]
realpath class: Pathname
realdirpath class: Pathname
children classes: [Pathname]
children sorted: ["deep"]
children bare: ["deep"]
each_child: ["deep"]
entries sorted: [".", "..", "deep"]
each_entry: [".", "..", "deep"]
glob: ["hello.txt", "new.txt", "t.txt", "w.txt"]
glob block: ["hello.txt", "new.txt", "t.txt", "w.txt"]
find: [".", "empty", "hard", "hello.txt", "link", "new.txt", "sub", "sub/deep", "sub/deep/leaf.txt", "t.txt", "w.txt"]
mkdir: true
mkdir twice: "Errno::EEXIST: File exists"
rmdir: false
mkpath deep: true
rmtree: false
opendir: Dir
mountpoint? tmp: false
mountpoint? root: true
getwd class: Pathname
pwd agrees with Dir: true
Pathname.glob class: Array
