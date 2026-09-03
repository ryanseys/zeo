# `SystemCallError.===` -- what `rescue Errno::ENOENT` really asks. It matches
# on the ERRNO NUMBER, so any object answering that number matches, and the
# bare `SystemCallError` matches every one of its instances outright.
#
# zeo answered this off `Module#===` (a plain kind-of test), which is right for
# the two common cases and wrong for the duck-typed one.

# Asked by inclusion, not by the whole list: zeo registers `exception` and
# `to_tty?` on every exception id (flat dispatch), so its list carries two
# names ruby keeps on `Exception` alone.
p SystemCallError.singleton_methods(false).include?(:===)
p SystemCallError.method(:===).arity

# The bare class matches any instance; a specific one matches its own number.
p SystemCallError === SystemCallError.new("m", 2)
p SystemCallError === Errno::EACCES.new
p Errno::ENOENT === Errno::ENOENT.new
p Errno::ENOENT === Errno::EACCES.new
p Errno::ENOENT === SystemCallError.new("m", Errno::ENOENT::Errno)

# Anything that is not a SystemCallError and cannot answer `errno` misses.
p SystemCallError === "x"
p Errno::ENOENT === "x"
p Errno::ENOENT === nil

# ... and anything that CAN answer it matches by number alone.
class Faker
  def initialize(n) = @n = n
  def errno = @n
end
p Errno::ENOENT === Faker.new(Errno::ENOENT::Errno)
p Errno::ENOENT === Faker.new(Errno::EACCES::Errno)
# On `SystemCallError` the name `Errno` reaches the top-level MODULE, which
# equals no integer, so a duck never matches the bare class.
p SystemCallError === Faker.new(Errno::ENOENT::Errno)

# The whole point: a real rescue still selects the right handler.
begin
  File.read("/definitely/not/here")
rescue Errno::EACCES
  p :wrong
rescue Errno::ENOENT => e
  p [e.class, e.errno]
end

begin
  raise Errno::EACCES
rescue SystemCallError => e
  p e.class
end

# And a `case` over the same objects, which is `===` by another spelling.
[Errno::ENOENT.new, Errno::EACCES.new, RuntimeError.new].each do |e|
  case e
  when Errno::ENOENT then p :enoent
  when SystemCallError then p :other_syscall
  else p :not_syscall
  end
end
__END__
true
1
true
true
true
false
true
false
false
false
true
false
false
[Errno::ENOENT, 2]
Errno::EACCES
:enoent
:other_syscall
:not_syscall
