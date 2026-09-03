# A `.so` require resolves through the static-ext table rather than the
# filesystem -- zeo is a Ruby built with `--with-static-linked-ext`, and
# CRuby registers static exts under `"<feature>.so"` (`load.c:1161`).
#
# A .so naming no static ext resolves to nothing, so (like any unresolvable
# plain require) it lowers to a runtime `Kernel#require` raising LoadError --
# it COMPILES rather than failing the build.

require "strscan.so"
p StringScanner.new("ab").scan(/a/)
__END__
"a"
