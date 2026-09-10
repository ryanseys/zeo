# Dir singleton methods: mkdir / rmdir / exist? round-trip, glob (sorted
# in Ruby 3.0+), and home. Uses a cwd-relative directory (not /tmp) so it
# works on the Windows MinGW build too; the test runs in a fresh temp cwd
# and removes everything it creates.
d = "sptest_probe_dir"
Dir.mkdir(d) unless Dir.exist?(d)
puts Dir.exist?(d)
File.write("#{d}/b.txt", "x")
File.write("#{d}/a.txt", "y")
puts Dir.glob("#{d}/*.txt").join(",")
File.delete("#{d}/a.txt")
File.delete("#{d}/b.txt")
Dir.rmdir(d)
puts Dir.exist?(d)
puts(Dir.home.length > 0)
__END__
true
sptest_probe_dir/a.txt,sptest_probe_dir/b.txt
false
true
