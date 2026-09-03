# `OpenSSL::Buffering` is a real module, mixed into SSLSocket, that supplies
# the whole read/write surface over the receiver's `sysread`/`syswrite`/
# `sysclose`. It used to be implemented straight onto SSLSocket, so the module
# could not be named, reopened, or asked about its ancestry.
require "openssl"
require "socket"

puts "-- the module --"
p OpenSSL::Buffering.instance_of?(Module)
p OpenSSL::Buffering::BLOCK_SIZE
p OpenSSL::Buffering.include?(Enumerable)

puts "-- who owns what --"
p OpenSSL::SSL::SSLSocket.include?(OpenSSL::Buffering)
p OpenSSL::SSL::SSLSocket.include?(OpenSSL::SSL::SocketForwarder)
p OpenSSL::SSL::SSLSocket.ancestors.include?(Enumerable)
%i[gets read readpartial puts print each_line ungetc eof? sync].each do |m|
  p [m, OpenSSL::SSL::SSLSocket.instance_method(m).owner]
end
%i[sysread syswrite sysclose connect hostname context].each do |m|
  p [m, OpenSSL::SSL::SSLSocket.instance_method(m).owner]
end
%i[addr peeraddr fileno closed? getsockopt].each do |m|
  p [m, OpenSSL::SSL::SSLSocket.instance_method(m).owner]
end

puts "-- an unconnected socket carries the buffering ivars --"
srv = TCPServer.new("127.0.0.1", 0)
sock = TCPSocket.new("127.0.0.1", srv.addr[1])
ssl = OpenSSL::SSL::SSLSocket.new(sock)
p ssl.instance_variables.sort
p ssl.instance_variable_get(:@io).equal?(sock)
p ssl.sync
sock.close
srv.close

# The module knows nothing about TLS, so anything answering the three
# unbuffered primitives can mix it in. This is the whole surface, driven off
# a String rather than a socket.
class Tape
  include OpenSSL::Buffering

  # The module's own `initialize` seeds these from an `@io`; a Tape has none,
  # so it seeds them itself.
  def initialize(text)
    @tape = text.dup
    @written = +""
    @rbuffer = +""
    @eof = false
    @sync = true
  end

  attr_reader :written

  def sysread(len = 16384, buf = nil)
    raise EOFError, "end of file reached" if @tape.empty?
    @tape.slice!(0, len)
  end

  def syswrite(s)
    @written << s.to_s
    s.to_s.bytesize
  end

  def sysclose
    @tape = +""
    :closed
  end
end

puts "-- reading --"
p Tape.new("one\ntwo\nthree\n").gets
p Tape.new("one\ntwo\nthree\n").readlines
p Tape.new("one\ntwo\nthree\n").read
p Tape.new("abcdef").read(3)
p Tape.new("abcdef").readpartial(2)
p Tape.new("abc").getc
p Tape.new("abc").getbyte
p Tape.new("").gets
p Tape.new("").read
p Tape.new("").eof?
p Tape.new("x").eof?

puts "-- line by line --"
lines = []
Tape.new("a\nb\nc\n").each_line { |l| lines << l }
p lines
bytes = []
Tape.new("hi").each_byte { |b| bytes << b }
p bytes

puts "-- pushback --"
t = Tape.new("bcdef")
p t.getc
t.ungetc("a")
p t.read

puts "-- end of stream --"
t = Tape.new("only")
p t.read
begin
  t.readpartial(4)
rescue EOFError => e
  p [:readpartial, e.class]
end
begin
  Tape.new("").readline
rescue EOFError => e
  p [:readline, e.class]
end
begin
  Tape.new("").readchar
rescue EOFError => e
  p [:readchar, e.class]
end

puts "-- writing --"
t = Tape.new("")
p t.write("ab", "cd")
t.print("ef")
t.puts("gh")
t.printf("%03d", 7)
t << "!"
p t.written
p t.flush.equal?(t)
p Tape.new("").close
__END__
-- the module --
true
16384
true
-- who owns what --
true
true
true
[:gets, OpenSSL::Buffering]
[:read, OpenSSL::Buffering]
[:readpartial, OpenSSL::Buffering]
[:puts, OpenSSL::Buffering]
[:print, OpenSSL::Buffering]
[:each_line, OpenSSL::Buffering]
[:ungetc, OpenSSL::Buffering]
[:eof?, OpenSSL::Buffering]
[:sync, OpenSSL::Buffering]
[:sysread, OpenSSL::SSL::SSLSocket]
[:syswrite, OpenSSL::SSL::SSLSocket]
[:sysclose, OpenSSL::SSL::SSLSocket]
[:connect, OpenSSL::SSL::SSLSocket]
[:hostname, OpenSSL::SSL::SSLSocket]
[:context, OpenSSL::SSL::SSLSocket]
[:addr, OpenSSL::SSL::SocketForwarder]
[:peeraddr, OpenSSL::SSL::SocketForwarder]
[:fileno, OpenSSL::SSL::SocketForwarder]
[:closed?, OpenSSL::SSL::SocketForwarder]
[:getsockopt, OpenSSL::SSL::SocketForwarder]
-- an unconnected socket carries the buffering ivars --
[:@context, :@eof, :@io, :@rbuffer, :@sync]
true
true
-- reading --
"one\n"
["one\n", "two\n", "three\n"]
"one\ntwo\nthree\n"
"abc"
"ab"
"a"
97
nil
""
true
false
-- line by line --
["a\n", "b\n", "c\n"]
[104, 105]
-- pushback --
"b"
"acdef"
-- end of stream --
"only"
[:readpartial, EOFError]
[:readline, EOFError]
[:readchar, EOFError]
-- writing --
4
"abcdefgh\n007!"
true
:closed
