# File.foreach's return value with and without a block, Dir.open's block
# children, Method objects for the boolean operators, a deferred handler's
# captures, NoMethodError#args, and find_index with a destructured pair.
require "tmpdir"
ZTMP = Dir.mktmpdir

path = File.join(ZTMP, "sp_ff")
File.write(path, "hello\nworld\n")
v = File.foreach(path) { |line| line }; p v
fe = File.foreach(path).class;           p fe
acc = []
File.foreach(path) { |l| acc << l }
p acc
File.delete(path)
d = File.join(ZTMP, "sp_dir34")
Dir.mkdir(d) unless Dir.exist?(d)
File.write("#{d}/x", "")
a = []; Dir.open(d) { |dir| a = dir.children }
p a.sort
File.delete("#{d}/x"); Dir.rmdir(d)
r1 = (true.method(:&).call(false) rescue $!.class); p r1
r2 = (false.method(:|).call(true) rescue $!.class); p r2
r3 = (true.public_method(:^).call(true) rescue $!.class); p r3
r4 = (true.method(:nope) rescue $!.class); p r4
begin; nil.foo(1, 2); rescue NoMethodError => e; p e.args; p e.message; end
begin; nil.bar; rescue NoMethodError => e; p e.args; end
x = [1, "s"].sample
begin; x.zzz(:a, "b") if false; nil.qq(9); rescue NoMethodError => e; p e.args; end
a = [["x", 1], ["y", 2]]
p a.find_index { |k, v| k == "y" }
__END__
nil
Enumerator
["hello\n", "world\n"]
["x"]
false
true
false
NameError
[1, 2]
"undefined method 'foo' for nil"
[]
[9]
1
