# sp_argv stores `const char *` pointers into the str-heap (via
# sp_str_dup_external on main entry). Without explicit GC marking,
# sp_str_sweep reaps them on the first collect, leaving sp_argv.data
# entries as dangling pointers that any later `ARGV[i]` read
# dereferences.
#
# Triggering the bug requires:
#  - Reading ARGV[i] (just the read, not even a method call).
#  - GC pressure that actually fires sp_gc_collect — string allocs
#    don't accrue to sp_gc_bytes per sp_str_alloc's threshold note,
#    so we need object allocations to push past 256 KB.
#  - Reading ARGV[i] again after GC; the dangling pointer either
#    segfaults outright or yields freed memory.
#
# The fix marks sp_argv.data[*] alongside the regex globals in
# sp_re_mark_globals, so sp_str_sweep keeps argv strings alive.
# argv_gc.rb.args feeds five short args so the harness exercises a
# non-empty ARGV (short strings hit the segfault path under the bug).

class Trash
  def initialize(n)
    @n = n
    @s = "padding payload " * 64    # ~1 KB heap string per Trash
  end
  attr_reader :n
end

# Allocate ~5000 Trash instances (~5 MB) — far past the 256 KB GC
# threshold — to force sp_gc_collect / sp_str_sweep cycles.
junk = []
j = 0
while j < 5000
  junk << Trash.new(j)
  j = j + 1
end
puts junk.length     # 5000

# Read ARGV[i] after GC. With the bug, sp_argv.data[i] points to
# freed memory; reading it (and its length) crashes or returns
# garbage.
i = 0
while i < ARGV.length
  puts ARGV[i] + ":" + ARGV[i].length.to_s
  i = i + 1
end
