# Pull in the statically linked native half FIRST, so `PTY` exists to be
# reopened below. This is CRuby's loader idiom -- see
# `ext/strscan/lib/strscan.rb` for the same shape and the reason for it.
require "pty.so"

module PTY
  # CRuby defines this in C (`ext/pty/pty.c`) with a `#status` reader its
  # constructor fills; a feature-gated native class cannot register a
  # constructible exception in this runtime (see `ext/zlib/lib/zlib.rb` for
  # the row-shape reason), so it lives here. `PTY.check` raises it by name and
  # then stamps `@status` onto the object it built, so this reader answers the
  # real `Process::Status`.
  class ChildExited < RuntimeError
    attr_reader :status
  end
end
