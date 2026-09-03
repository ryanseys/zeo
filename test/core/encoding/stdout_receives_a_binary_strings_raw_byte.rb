# The harness captures stdout lossily, so the RAW 0xB4 byte surfaces
# as one U+FFFD replacement char -- whereas a promoted output
# (0xC2 0xB4) would decode "cleanly" as U+00B4. The replacement char IS
# the proof the byte reached the fd untouched; the byte-count return
# (1, not 2) pins it from a second angle.

n = $stdout.write(180.chr)
puts
p n
__END__
´
1
