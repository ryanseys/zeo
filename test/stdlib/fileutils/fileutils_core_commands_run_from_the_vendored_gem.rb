# `require "fileutils"` compiles the real upstream gem the lock resolves,
# including its load-time module metaprogramming (module_function, extend
# self, class << self, a platform-conditional StreamUtils_), and the core
# file operations bundler relies on work.

require "fileutils"
puts FileUtils::VERSION
puts FileUtils.respond_to?(:mkdir_p)
d = "/tmp/zeo_fu_e2e_#{Process.pid}"
FileUtils.rm_rf(d)
FileUtils.mkdir_p("#{d}/a/b")
puts Dir.exist?("#{d}/a/b")
File.write("#{d}/a/f", "hi")
FileUtils.cp("#{d}/a/f", "#{d}/a/g")
puts File.read("#{d}/a/g")
FileUtils.mv("#{d}/a/g", "#{d}/a/h")
puts File.exist?("#{d}/a/h")
FileUtils.rm_rf(d)
puts Dir.exist?(d)
__END__
1.8.0
true
true
hi
true
false
