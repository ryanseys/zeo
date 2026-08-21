# Output that does NOT end in a newline is the whole point of this file.
# Stdout is line-buffered, so a newline-terminated program flushes itself
# on the way through and can never see the bug: an AOT binary RETURNS its
# status to the emitted C `main`, past the cleanup Rust runs from
# `lang_start`, so nothing wrote the buffer out and the tail was lost.
# `bm_so_mandelbrot` (a PBM writer) is what caught it, at 44,590 bytes of
# 45,011 -- and every one of the suite's 5,000 newline-terminated goldens
# passed over it. Run this one through the AOT leg.
print "A" * 300
print "B" * 300
print "-no trailing newline"
