#@ only: macos
# `SocketError` and `Socket::ResolutionError` are C classes in CRuby
# (`ext/socket/init.c`). A feature-gated native class here cannot register a
# constructible exception, so they live in the gem's Ruby half -- the shape
# `strscan` and `zlib` already use. Without them a resolution failure reached
# `raise_error` with a class nothing had registered, which PANICS and ends the
# process rather than raising.
require "socket"

p SocketError.superclass
p Socket::ResolutionError.superclass
p Socket::ResolutionError.ancestors.include?(StandardError)

# CRuby's own class for a `getaddrinfo` failure since 3.4, and it inherits
# `SocketError`, so the older `rescue SocketError` still catches it.
begin
  Socket.getaddrinfo("nope.invalid.example", nil)
rescue SocketError => e
  p e.class
  p e.message
  p e.error_code.is_a?(Integer)
end
__END__
StandardError
SocketError
true
Socket::ResolutionError
"getaddrinfo: nodename nor servname provided, or not known"
true
