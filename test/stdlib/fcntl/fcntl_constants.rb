# `require "fcntl"` -- the Fcntl module is a constant table and nothing else;
# the operations that use these are `IO#fcntl`, which is IO's.
#
# The VALUES are the platform's own (`O_NOCTTY` is 0x20000 on Darwin and 0x100
# on Linux), so this asserts only the ones POSIX fixes plus the relationships
# that hold everywhere -- the same reason CRuby reads them from the headers
# rather than writing them down.
require "fcntl"

p Fcntl::VERSION

# The `fcntl(2)` commands POSIX numbers.
p [Fcntl::F_DUPFD, Fcntl::F_GETFD, Fcntl::F_SETFD, Fcntl::F_GETFL, Fcntl::F_SETFL]
p Fcntl::FD_CLOEXEC

# The lock commands and lock types vary; what holds is that they exist, are
# Integers, and don't collide.
locks = [Fcntl::F_GETLK, Fcntl::F_SETLK, Fcntl::F_SETLKW]
p locks.all?(Integer), locks.uniq.size
types = [Fcntl::F_RDLCK, Fcntl::F_WRLCK, Fcntl::F_UNLCK]
p types.all?(Integer), types.uniq.size

# `open(2)` access modes, which POSIX also fixes.
p [Fcntl::O_RDONLY, Fcntl::O_WRONLY, Fcntl::O_RDWR, Fcntl::O_ACCMODE]

# `O_NDELAY` is the historical spelling of `O_NONBLOCK` and aliases it.
p Fcntl::O_NDELAY == Fcntl::O_NONBLOCK

# The status flags are distinct single bits.
flags = [Fcntl::O_CREAT, Fcntl::O_EXCL, Fcntl::O_TRUNC, Fcntl::O_APPEND, Fcntl::O_NONBLOCK]
p flags.uniq.size
p flags.all? { |f| f > 0 && (f & (f - 1)) == 0 }

# ...and they are the same numbers `File::Constants` carries, which is where
# the core has them.
p Fcntl::O_APPEND == File::APPEND
p Fcntl::O_CREAT == File::CREAT

# The point of the table: reading a descriptor's flags back through `IO#fcntl`.
r, w = IO.pipe
p (r.fcntl(Fcntl::F_GETFL) & Fcntl::O_ACCMODE) == Fcntl::O_RDONLY
p (w.fcntl(Fcntl::F_GETFL) & Fcntl::O_ACCMODE) == Fcntl::O_WRONLY
p r.fcntl(Fcntl::F_GETFD).is_a?(Integer)
r.close
w.close

# Unrequired, the constant is not there at all -- but this file required it, so
# what is observable here is that the module reports itself as one.
p Fcntl.class
p Fcntl.name
__END__
"1.3.0"
[0, 1, 2, 3, 4]
1
true
3
true
3
[0, 1, 2, 3]
true
5
true
true
true
true
true
true
Module
"Fcntl"
