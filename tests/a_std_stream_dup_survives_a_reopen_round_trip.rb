# `$stdout.dup` has to be a REAL descriptor, not a second name for fd 1:
# saving the old stream before a `reopen` and restoring it afterwards is the
# whole point, and mkmf's `Logging.open` is that exact sequence.
log = File.open("round-trip.log", "wb")
saved = $stdout.dup
puts "a fresh descriptor: #{saved.fileno != $stdout.fileno}"
puts "a plain IO:         #{saved.class}"

$stdout.reopen(log)
puts "into the log"
$stdout.reopen(saved)
puts "back on the console"

log.close
saved.close
puts "the log holds: #{File.read("round-trip.log").split("\n").inspect}"
File.unlink("round-trip.log")

# A dup of a closed stream still raises, and a File dup still reads the file.
File.write("payload.txt", "abc")
f = File.open("payload.txt")
g = f.dup
puts "a File dup reads:   #{g.read}"
f.close
g.close
File.unlink("payload.txt")
