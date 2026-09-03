# Ported from spinel's bundled sp_net.c FFI surface. sp_net_shell_capture is
# spinel-private (Kernel backticks are the Ruby spelling of the same thing);
# getpid stays a real FFI binding through the ffi gem.
#
# Cross-platform-deterministic smoke: shell capture + getpid produce the
# same output on every POSIX target, so the .expected holds for Linux +
# macOS CI.
require "ffi"

module Net
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :getpid, [], :int
end

# Shell capture: stdout of `printf hello` is exactly "hello".
puts `printf hello`

# getpid is always a positive pid (print a stable token, not the pid).
puts(Net.getpid > 0 ? "pid-ok" : "pid-bad")
__END__
hello
pid-ok
