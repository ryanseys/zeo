# DECIDED DIVERGENCE. `IO#each_byte` on a write-only handle answers
# `IOError: not opened for reading` here, where CRuby answers
# `Errno::EBADF: Bad file descriptor @ io_fillbuf`.
#
# CRuby is inconsistent with itself at this one row: every other reader
# checks the mode first and names it, and `each_byte` alone falls through
# to `io_fillbuf` and reports what the kernel said. Matching it means
# giving `each_byte` a per-byte descriptor path whose only purpose is to
# surface a raw errno -- worse code for a worse message, at the row where
# `tests/the_mode_is_checked_before_the_descriptor.rb` records that the
# other seventeen agree.
#
# The classes differ, so a program rescuing `Errno::EBADF` here would see
# `IOError` instead. Both are errors and both name the same cause.

require "tmpdir"

Dir.mktmpdir do |dir|
  path = File.join(dir, "f")
  File.write(path, "abcd")
  File.open(path, "w") do |f|
    begin
      f.each_byte { |_| }
    rescue Exception => e
      puts "#{e.class}: #{e.message.sub(/ - .*/, '')}"
    end
  end
end
__END__
IOError: not opened for reading
