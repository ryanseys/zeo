# Pathname's private path-algebra helpers -- the exact surface
# pathname_builtin.rb declares, oracle-pinned shapes.
pn = Pathname.new("x")

p pn.send(:chop_basename, "a/b/c")
p pn.send(:chop_basename, "a/b/")
p pn.send(:chop_basename, "//")
p pn.send(:chop_basename, "")
p pn.send(:split_names, "/a/b/c")
p pn.send(:split_names, "a//b/")
p pn.send(:prepend_prefix, "", "a/b")
p pn.send(:prepend_prefix, "/", "a/b")
p pn.send(:prepend_prefix, "/x/", "a")
p pn.send(:prepend_prefix, "/", "")
p pn.send(:has_trailing_separator?, "a/")
p pn.send(:has_trailing_separator?, "a")
p pn.send(:has_trailing_separator?, "/")
p pn.send(:add_trailing_separator, "a")
p pn.send(:add_trailing_separator, "a/")
p pn.send(:add_trailing_separator, "")
p pn.send(:del_trailing_separator, "a//")
p pn.send(:del_trailing_separator, "///")
p pn.send(:del_trailing_separator, "")
p pn.send(:same_paths?, "A", "a")
p pn.send(:same_paths?, "a", "a")
p pn.send(:plus, "a/b/c", "../../d")
p pn.send(:plus, "a", "/z")
p pn.send(:plus, "..", "..")
p Pathname.new("a/../b/./c").send(:cleanpath_aggressive)
p Pathname.new("/../a").send(:cleanpath_aggressive)
p Pathname.new("a/.").send(:cleanpath_conservative)
p Pathname.new("a/b/").send(:cleanpath_conservative)
p Pathname.new("./").send(:cleanpath_conservative)

# The `.` tail survives the PUBLIC conservative cleanpath too.
p Pathname.new("a/.").cleanpath(true)

# All ten are private.
priv = Pathname.private_instance_methods(false)
p %i[chop_basename split_names prepend_prefix has_trailing_separator?
     add_trailing_separator del_trailing_separator same_paths? plus
     cleanpath_aggressive cleanpath_conservative].all? { |m| priv.include?(m) }
