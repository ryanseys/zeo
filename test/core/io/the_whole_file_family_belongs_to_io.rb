# Who owns read, write, binread, binwrite, readlines and foreach.
FAMILY = %i[binread binwrite foreach read readlines write].freeze
p (File.singleton_methods(false) & FAMILY).sort
p (IO.singleton_methods(false) & FAMILY).sort
p FAMILY.map { File.method(_1).owner }.uniq
p FAMILY.map { IO.method(_1).owner }.uniq
path = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_family_#{Process.pid}.txt")
File.write(path, "a\nb\n")
p File.read(path), File.binread(path, 1), File.readlines(path, chomp: true)
IO.write(path, "c\n")
p IO.read(path)
File.delete(path)
__END__
[]
[:binread, :binwrite, :foreach, :read, :readlines, :write]
[#<Class:IO>]
[#<Class:IO>]
"a\nb\n"
"a"
["a", "b"]
"c\n"
