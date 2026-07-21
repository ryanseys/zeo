# File.expand_path: pure-string absolute-path expansion. Only absolute
# bases/paths are exercised so the result is independent of cwd/HOME.
puts File.expand_path("test", "/tmp")
puts File.expand_path("/a/b/c")
puts File.expand_path("../x", "/a/b")
puts File.expand_path("./y", "/a/b")
puts File.expand_path("a/../b/./c", "/root")
puts File.expand_path("", "/tmp")
puts File.expand_path("/")
puts File.expand_path("p/q/../../r", "/base")
