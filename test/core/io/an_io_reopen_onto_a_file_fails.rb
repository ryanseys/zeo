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
__END__
[:missing_w, [true, "created"]]
[:existing_w, "fresh"]
[:existing_bare, "bare"]
[:io_to_io, "dup2"]
:devnull_ok
