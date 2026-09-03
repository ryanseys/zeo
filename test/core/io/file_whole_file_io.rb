# Whole-file read/write/readlines, including `chomp: true`.

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_rw_#{Process.pid}")
Dir.mkdir(dir)
begin
  path = File.join(dir, "lines.txt")
  n = File.write(path, "alpha\nbeta\ngamma\n")
  p n
  p File.read(path)
  p File.readlines(path)
  p File.readlines(path, chomp: true)
  File.write(path, "no trailing newline")
  p File.readlines(path)
  File.delete(path)
ensure
  Dir.rmdir(dir)
end
__END__
17
"alpha\nbeta\ngamma\n"
["alpha\n", "beta\n", "gamma\n"]
["alpha", "beta", "gamma"]
["no trailing newline"]
