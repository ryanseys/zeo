# A user class inheriting a native type has no struct of its own: its instances
# wrap the real native value as a payload and carry their own ivars beside it.
# `Queue` has a real empty form, so `super()` builds one; nothing in the IO
# family does -- no descriptor, peer or algorithm can be conjured -- so those
# subclasses seat the real payload through `super`, which is the whole reason
# gems subclass them.
#
# actionpool's `Queue < ::Queue`, kgio's `Kgio::Pipe < IO`, dalli's
# `TCP < TCPSocket`, and the `SSLSocket < OpenSSL::SSL::SSLSocket` that bunny,
# dalli and mongo each write -- the largest single subclassing bucket in the
# gem corpus.

require "socket"
require "openssl"
require "tmpdir"

# A real empty form: `super()` builds the queue, and the subclass adds an ivar
# on top of the inherited behaviour.
class Jobs < ::Queue
  def initialize(name)
    super()
    @name = name
  end

  attr_reader :name

  def push_all(*xs) = xs.each { |x| push(x) }
end

q = Jobs.new("work")
q.push_all(1, 2, 3)
p q.name
p q.size
p q.pop
p q.class
p q.is_a?(Queue)
p Jobs.superclass
p Jobs.ancestors.take(3)

# The `File` shape: the payload arrives through `super`, and every inherited
# method reads it. `IO.sysopen` hands over a descriptor nobody else owns.
class Tagged < IO
  def initialize(fd, tag)
    super(fd)
    @tag = tag
  end

  attr_reader :tag

  def first_line = "#{@tag}: #{gets}"
end

Dir.mktmpdir do |dir|
  path = File.join(dir, "log")
  File.write(path, "alpha\nbeta\n")
  io = Tagged.new(IO.sysopen(path), :note)
  p io.class
  p io.tag
  p io.is_a?(IO)
  p io.first_line
  p io.read
  io.close
  p io.closed?
end

p Tagged.superclass

# Deeper in the same family: the payload root is the NEAREST native ancestor,
# so a `TCPSocket` subclass is a TCPSocket, not the `IO` four links above it.
class Named < TCPSocket
  def initialize(host, port, name)
    super(host, port)
    @name = name
  end

  attr_reader :name
end

p Named.superclass
p Named.ancestors.include?(IO)
p Named.instance_method(:name).owner

# A socket wrapper, and a cipher: both named at construction, both the same
# shape.
class Wrapped < OpenSSL::SSL::SSLSocket
  def wrapped? = true
end

p Wrapped.superclass
p Wrapped.new.wrapped? rescue p :needs_a_socket

class Loud < OpenSSL::Cipher
  def shout = "LOUD"
end

p Loud.superclass
c = Loud.new("aes-256-cbc")
p c.class
p c.shout
p c.name
p c.is_a?(OpenSSL::Cipher)

class Named2 < OpenSSL::Digest
  def label = "#{name} digest"
end

d = Named2.new("SHA256")
p Named2.superclass
p d.class
p d.label
p d.hexdigest("abc")

# `Fiber` is `Thread`'s shape: the block IS the body, so `super(&block)` is the
# only way to seat one. hexapdf's `FiberWithLength` records a length it knows
# before the fiber runs.
class Measured < Fiber
  def initialize(length, &block)
    super(&block)
    @length = length || -1
  end

  attr_reader :length
end

f = Measured.new(3) { Fiber.yield(:a); Fiber.yield(:b); :done }
p Measured.superclass
p f.class
p f.length
p f.resume
p f.resume
p f.resume
p f.alive?
