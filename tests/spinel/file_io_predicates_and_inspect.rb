# File real-uid predicates, File.realdirpath, IO#inspect, String/IO.try_convert
# Everything is compared against a path this test creates and resolves itself:
# /tmp and /etc are symlinks on macOS, so a hardcoded expectation is not portable.
dir = "spinel_rdp_test_dir"
Dir.mkdir(dir) unless Dir.exist?(dir)
path = File.join(dir, "sample.txt")
File.write(path, "hello\n")
real_dir = File.realpath(dir)

p File.readable_real?(path)
p File.readable_real?("no_such_spinel_file_xyz")
p File.writable_real?("no_such_spinel_file_xyz")
p File.executable_real?("no_such_spinel_file_xyz")
p File.readable?(path)
p File.readable?("no_such_spinel_file_xyz")

p File.realdirpath(dir) == real_dir
p File.realdirpath(File.join(dir, "missing_name_xyz")) == File.join(real_dir, "missing_name_xyz")
p File.realdirpath("missing_name_xyz", dir) == File.join(real_dir, "missing_name_xyz")

f = File.open(path)
p f.inspect == "#<File:#{path}>"
f.close
p f.inspect == "#<File:#{path} (closed)>"
p STDIN.inspect

p String.try_convert("abc")
p String.try_convert(5)
p Integer.try_convert(5)

p Math.expm1(0)
p Math.log1p(0)

File.delete(path)
Dir.rmdir(dir)
