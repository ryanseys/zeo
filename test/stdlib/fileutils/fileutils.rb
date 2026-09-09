# FileUtils -- the vendored pure-Ruby stdlib gem (gems/fileutils), compiled
# from its real upstream source, not reimplemented. Exercises the core file
# operations rubygems/bundler rely on; every line is oracle-matched.
require "tmpdir"
ZTMP = Dir.mktmpdir

require "fileutils"

puts FileUtils::VERSION
puts FileUtils.respond_to?(:mkdir_p)

d = File.join(ZTMP, "zeo_fileutils_example")
FileUtils.rm_rf(d) # a clean slate, idempotent across runs

FileUtils.mkdir_p("#{d}/a/b/c")
puts Dir.exist?("#{d}/a/b/c")

File.write("#{d}/a/src.txt", "hello")
FileUtils.cp("#{d}/a/src.txt", "#{d}/a/copy.txt")
puts File.read("#{d}/a/copy.txt")

FileUtils.mv("#{d}/a/copy.txt", "#{d}/a/moved.txt")
puts File.exist?("#{d}/a/moved.txt")
puts File.exist?("#{d}/a/copy.txt")

FileUtils.touch("#{d}/a/t.txt")
puts File.exist?("#{d}/a/t.txt")

FileUtils.rm_rf(d)
puts Dir.exist?(d)
__END__
1.8.0
true
true
hello
true
false
true
false
