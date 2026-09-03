# The `File.<predicate>?` family answers false for a missing path rather
# than raising -- and `size?` is nil for a missing OR empty file, though
# `size` is 0 for an empty one.

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_pred_#{Process.pid}")
Dir.mkdir(dir)
begin
  full = File.join(dir, "full.txt")
  empty = File.join(dir, "empty.txt")
  missing = File.join(dir, "missing.txt")
  File.write(full, "12345")
  File.write(empty, "")

  p [File.exist?(full), File.exist?(missing)]
  p [File.file?(full), File.file?(dir)]
  p [File.directory?(dir), File.directory?(full)]
  p [File.zero?(empty), File.zero?(full)]
  p File.size(full)
  p File.size(empty)
  p File.size?(full)
  p File.size?(empty)
  p File.size?(missing)
  p [File.exist?(missing), File.file?(missing), File.directory?(missing)]

  File.delete(full, empty)
ensure
  Dir.rmdir(dir)
end
__END__
[true, false]
[true, false]
[true, false]
[true, false]
5
0
5
nil
nil
[false, false, false]
