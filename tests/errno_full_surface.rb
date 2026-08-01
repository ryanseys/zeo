# `Errno` is generated from one table (`zeo_abi::ERRNO_CLASSES`), so every name
# the platform errno list carries exists -- not just the dozen the runtime
# happened to raise. A name the platform does not define is bound to
# `Errno::NOERROR`, and a second spelling of one value is bound to the class
# that owns it, both exactly as CRuby does.

puts "-- every name is present, and they are the platform's own"
p Errno.constants.size
p Errno.constants.include?(:ERANGE)
p Errno.constants.include?(:EWOULDBLOCK)
p Errno::ERANGE.name

puts "-- each class names the errno it stands for"
p Errno::ENOENT::Errno
p Errno::EACCES::Errno
p Errno::EINVAL::Errno
p Errno::ENOENT.new.errno

puts "-- a second spelling IS the class it duplicates"
p Errno::EWOULDBLOCK.equal?(Errno::EAGAIN)
p Errno::EWOULDBLOCK.name
begin
  raise Errno::EAGAIN
rescue Errno::EWOULDBLOCK => e
  p ["EWOULDBLOCK caught an EAGAIN", e.class]
end

puts "-- a name this platform lacks is NOERROR, as in CRuby"
p Errno::EADV.name
p Errno::NOERROR::Errno

puts "-- the whole hierarchy"
p Errno::ENOENT.superclass
p Errno::ENOENT.ancestors.first(4)

puts "-- the message is composed from strerror, not stored"
p Errno::ENOENT.new.message
p Errno::ENOENT.new(nil).message
p Errno::ENOENT.new("f").message

puts "-- SystemCallError.new picks the class its errno names"
p SystemCallError.new("x", 2).class
p SystemCallError.new("x", 2).message
p SystemCallError.new("x", 2).errno
p SystemCallError.new(2).class
p SystemCallError.new(2).message

puts "-- and stays itself when the errno names none"
p SystemCallError.new("x").class
p SystemCallError.new("x").message
p SystemCallError.new("x").errno
p SystemCallError.new("x", 9999).class
p SystemCallError.new("x", 9999).message
p SystemCallError.new("x", 9999).errno

puts "-- a real syscall failure keeps CRuby's own message shape"
begin
  File.open("/nope/nope")
rescue SystemCallError => e
  p [e.class, e.message, e.errno]
end
begin
  Dir.mkdir("/nope/nope")
rescue SystemCallError => e
  p [e.class, e.errno]
end
