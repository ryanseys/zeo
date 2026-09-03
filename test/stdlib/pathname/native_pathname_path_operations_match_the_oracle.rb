# `Pathname`: path manipulation (`+`/`join` resolve `..` lexically like
# CRuby's `plus`), predicates, and reflection. The last line pins that it
# does NOT include Comparable, however much its `#<=>` suggests it.

require "pathname"
p = Pathname.new("/usr/local/bin/ruby")
puts p.to_s
puts p.basename
puts p.dirname
puts p.extname.inspect
puts p.absolute?
puts (p + "..").to_s
puts (Pathname.new("/usr") / "bin").to_s
puts p.basename(".rb")
puts Pathname.new("foo/bar.txt").sub_ext(".md")
puts Pathname.new("/a/b/c").relative_path_from(Pathname.new("/a")).to_s
puts p == Pathname.new("/usr/local/bin/ruby")
puts p.is_a?(Comparable)
__END__
/usr/local/bin/ruby
ruby
/usr/local/bin
""
true
/usr/local/bin
/usr/bin
ruby
foo/bar.md
b/c
true
false
