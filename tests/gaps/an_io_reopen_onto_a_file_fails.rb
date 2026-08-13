# `IO#reopen` works for exactly one shape in zeo -- `$stderr.reopen(IO::NULL)`,
# which is why every gap file above uses that spelling and nothing else. Point
# it at an ordinary file and it fails:
#
#   reopen(missing_path, "w")   Errno::ENOENT   -- "w" implies O_CREAT, so
#                                                  CRuby CREATES the file
#   reopen(existing_path, "w")  Errno::EBADF
#   reopen(existing_path)       Errno::EBADF    -- the one-argument form
#   reopen(other_io)            Errno::EBADF    -- the IO-to-IO form
#
# The EBADF says the implementation is dup2-ing something it never opened,
# rather than opening the target first and dup2-ing THAT onto the receiver's
# descriptor; the ENOENT says the mode string never reaches the open flags.
#
# Found while trying to capture a ractor's abort report: `$stderr.reopen(path)`
# is the only way to see output a ractor's own thread writes to fd 2, and the
# probe silently read a STALE file left by the oracle run instead. A broken
# reopen is a quiet trap for exactly that reason -- the call raises where a
# test expects a redirect, or (worse) the reader picks up whatever was already
# on disk.
#
# NEIGHBOUR, not asserted here: `$stderr = StringIO.new` does not capture a
# THREAD's abort report under zeo either. CRuby writes that report through
# `rb_stderr`, which the assignment replaces, so the same program prints the
# banner on stdout under CRuby and on the process's stderr under zeo. Between
# the two, there is currently NO way for a zeo program to capture what its own
# threads report -- reassignment is ignored and reopen is broken.
require "tmpdir"

DIR = Dir.tmpdir
PROBE = File.join(DIR, "zeo_gap_reopen_#{Process.pid}")
SINK = File.join(DIR, "zeo_gap_reopen_sink_#{Process.pid}")

def try(label)
  p [label, yield]
rescue StandardError => e
  p [label, e.class.to_s]
ensure
  [PROBE, SINK].each { |f| File.delete(f) if File.exist?(f) }
end

# "w" onto a MISSING path creates it.
try(:missing_w) do
  out = File.open(SINK, "w")
  out.reopen(PROBE, "w")
  out.puts "created"
  out.flush
  [File.exist?(PROBE), File.read(PROBE).chomp]
end

# "w" onto an EXISTING path truncates it.
try(:existing_w) do
  File.write(PROBE, "stale")
  out = File.open(SINK, "w")
  out.reopen(PROBE, "w")
  out.puts "fresh"
  out.flush
  File.read(PROBE).chomp
end

# The one-argument form.
try(:existing_bare) do
  File.write(PROBE, "stale")
  out = File.open(SINK, "w")
  out.reopen(PROBE)
  out.puts "bare"
  out.flush
  File.read(PROBE).chomp
end

# The IO-to-IO form.
try(:io_to_io) do
  target = File.open(PROBE, "w")
  out = File.open(SINK, "w")
  out.reopen(target)
  out.puts "dup2"
  out.flush
  File.read(PROBE).chomp
end

# The one shape that DOES work, so a fix must not regress it.
$stderr.reopen(IO::NULL)
p :devnull_ok
