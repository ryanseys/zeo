# `require "io/console"` enables the IO console surface (IO#winsize).
# Without the require, `winsize` is a NoMethodError, as in CRuby. On a
# redirected stdout `winsize` raises Errno::ENOTTY, not `[0, 0]`.
require "io/console"
p STDOUT.winsize
__END__
#@ stderr
stdlib/io_console/require_io_console_winsize.rb:11:in 'IO#winsize': Inappropriate ioctl for device - <STDOUT> (Errno::ENOTTY)
	from stdlib/io_console/require_io_console_winsize.rb:11:in '<main>'
#@ exit 1
