# `OpenSSL::Buffering#read_nonblock` is aliased to `readpartial`, and that
# alias breaks its contract in three ways. The consequence is that an HTTPS
# request over a KEEP-ALIVE connection never returns -- not a timeout, a hang.
#
# The three divergences, which the module header in
# `ext/openssl/ssl/buffering.rs` already admits to:
#
#   1. it BLOCKS. `readpartial` waits for a byte; `read_nonblock` must answer
#      `:wait_readable` at once when none is ready.
#   2. it ignores `exception: false`, so the end of the stream raises EOFError
#      where CRuby answers `nil`.
#   3. it ignores the output-buffer argument, so it returns a fresh String
#      where CRuby fills and returns the caller's own object.
#
# `Net::BufferedIO#rbuf_fill` (net/protocol.rb) is built on exactly that
# contract:
#
#   case rv = @io.read_nonblock(BUFSIZE, tmp, exception: false)
#   when String then ...
#   when :wait_readable then io.wait_readable(@read_timeout) or raise ReadTimeout
#   when nil then raise EOFError
#   end while true
#
# With `Connection: close` the peer closes, the blocking read returns, and the
# program finishes by accident. With keep-alive nothing closes, so the read
# never returns and `@read_timeout` never applies. Measured against
# rubygems.org: `Connection: close` answers 200; keep-alive hangs forever.
#
# Plain `IO`, `TCPSocket` and `Socket` all answer the contract correctly, so
# they are the reference. Fixing this means splitting `read_nonblock` from
# `readpartial`: a real non-blocking read (the descriptor is blocking today,
# so it needs `O_NONBLOCK` around the call or an `SSL_pending` check first),
# the `:wait_readable` and `nil` answers, and the output buffer honoured.
#
# The module mixes into any object supplying the `sys*` primitives, which is
# what lets this run with no socket and no network. That reaches divergences
# 2 and 3 directly, and proves the first by omission: zeo never calls
# `sysread_nonblock` at all, because its `read_nonblock` IS `readpartial`.
#
# Blocks the gem probe's port to ruby, which fetches over HTTPS.

require "openssl"

# `read_nonblock` delegates to `sysread_nonblock` and reads `@rbuffer`, so a
# mix-in supplies both.
class Feed
  include OpenSSL::Buffering

  def initialize(chunks)
    @chunks = chunks
    @rbuffer = +""
    @eof = false
    @sync = true
  end

  def sysread_nonblock(maxlen, buf = nil, exception: true)
    if @chunks.empty?
      return nil unless exception

      raise EOFError, "end of file reached"
    end
    got = @chunks.shift
    return got unless got.nil?
    return :wait_readable unless exception

    raise IO::WaitReadable
  ensure
    if got.is_a?(String) && buf
      buf.replace(got)
    end
  end

  def sysread(maxlen, buf = nil) = sysread_nonblock(maxlen, buf)
  def syswrite(s) = s.bytesize
  def sysclose = nil
end

io = Feed.new(["ab"])
buf = +"seed"

rv = io.read_nonblock(16, buf, exception: false)
puts "data:  #{rv.inspect} reused_outbuf=#{rv.equal?(buf)}"

rv = io.read_nonblock(16, buf, exception: false)
puts "eof:   #{rv.inspect}"

begin
  io.read_nonblock(16)
  puts "raise: no exception"
rescue EOFError => e
  puts "raise: #{e.class}"
end
