# Pull in the statically linked native half FIRST, so `Socket` exists to be
# reopened below. This is CRuby's loader idiom -- see `gems/strscan/lib/strscan.rb`
# for the same shape and the reason for it.
require "socket.so"

# CRuby defines this in C (`ext/socket/init.c`), but a feature-gated native
# class cannot register a constructible exception in this runtime: an ABI row
# is gated-but-constructor-less, and the exception table is constructible-but-
# ungated, so no row shape is both. Defined here it is an ordinary user class
# -- registered under its name with a real constructor -- which the native
# half raises by name. Without it, a resolution failure reached `raise_error`
# with a class nothing had registered, which PANICS and ends the process.
class SocketError < StandardError; end

class Socket
  # Every `getaddrinfo` failure, and CRuby's own class for it since 3.4 --
  # `SocketError` is what it inherits, so the old `rescue SocketError` still
  # catches. `#error_code` is the `getaddrinfo` return code; the native half
  # sets it when it raises.
  class ResolutionError < SocketError
    attr_reader :error_code
  end
end
