# The open/lock/fnmatch flags are a MODULE that `IO` includes, not a bag of
# constants on `File`. That is what makes `IO::APPEND` and `File::APPEND` the
# same constant and what puts `File::Constants` in `IO.ancestors` -- a program
# that opens an IO with `File::RDONLY` is relying on exactly that sharing.

p File::Constants.constants.sort
p File::Constants.class
p IO.include?(File::Constants)
p File.include?(File::Constants)
p IO.ancestors.first(4)

# The same constant, reached three ways.
p [File::APPEND, IO::APPEND, File::Constants::APPEND].uniq.size
p File::RDONLY == IO::RDONLY

# ...including under a name computed at run time.
name = "LOCK_EX"
p [IO.const_get(name), File.const_get(name), File::Constants.const_get(name)].uniq.size

# The seek/wait constants stay IO's own -- CRuby does not put them in the
# shared module.
p [IO::SEEK_SET, IO::SEEK_CUR, IO::SEEK_END, IO::SEEK_HOLE, IO::SEEK_DATA]
p [IO::READABLE, IO::PRIORITY, IO::WRITABLE]
p File::Constants.constants.include?(:SEEK_SET)

# Regexp's two encoding option bits, which zeo never sets but a program may
# still AND against.
p [Regexp::FIXEDENCODING, Regexp::NOENCODING]
p(/a/.options & Regexp::FIXEDENCODING)

# And the flags still do their job.
File.open(File::NULL, File::RDONLY) { |f| p f.class }
p File::NULL == IO::NULL
