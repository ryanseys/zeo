# require-gate: `require "io/console"` enables the IO console surface
# (IO#winsize). Without the require, winsize is unresolved under
# SPINEL_REQUIRE_GATE (matching CRuby's NoMethodError); with it, the
# C-native sp_File_winsize is provided. Locks the enable path (works gate on
# or off).
#
# The header above is spinel's; its claim that a redirected stdout reports
# [0, 0] is spinel's behaviour, not ruby's. ruby raises Errno::ENOTTY, which
# is what this test's re-oracled golden records and what zeo now does.
require "io/console"
p STDOUT.winsize
__END__
#@ stderr
stdlib/io_console/require_io_console_winsize.rb:11:in 'IO#winsize': Inappropriate ioctl for device - <STDOUT> (Errno::ENOTTY)
	from stdlib/io_console/require_io_console_winsize.rb:11:in '<main>'
#@ exit 1
