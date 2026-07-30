# A standard stream stored into a GC-scanned slot.
#
# The three standard streams are singletons in static storage, not GC
# allocations. Reads of $stdout / $stderr compile straight to the accessor and
# only ever dereference the result, so nothing scanned ever held one -- until
# an assignment stores it into a real global slot, which the globals hook
# marks. The collector decides what a pointer is from the byte before it, found
# whatever .bss happened to sit there, fabricated a header and called the scan
# pointer read out of it.
#
# Whether that faults is luck: the byte before is whatever the linker put
# there. This test is deterministic anyway -- SPINEL_GC_VERIFY names the object
# on the mark path regardless of whether the walk survives.

$stderr = $stdout
GC.start
puts "stderr assigned"

$stdout = $stderr
GC.start
puts "stdout assigned"

# a collection with real garbage to sweep, so the mark walk is not trivial
1000.times { |i| s = "row #{i}" ; s.upcase }
GC.start
puts "after churn"

# the streams still work as streams
$stderr.puts "written"
$stdout.flush
p $stderr == $stdout

# a stream held in an ordinary local and an array survives a collection too:
# the same pointer, now reachable from a scanned container
out = $stdout
box = [$stdout, $stderr]
GC.start
out.puts "through a local"
p box.length
