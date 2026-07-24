# sp_file_basename used to return `path + offset` — a pointer into
# the input path. Without keeping the underlying buffer rooted, the
# GC could free the path string, leaving the held basename dangling.
#
# Triggering the bug requires:
#  - A path constructed dynamically (heap string, not literal).
#  - A basename returned from a helper, so the path goes out of scope.
#  - GC pressure that actually fires `sp_gc_collect` so `sp_str_sweep`
#    walks the string heap and reaps the unreferenced path. String
#    allocations alone don't accrue to `sp_gc_bytes` (per the comment
#    in `sp_str_alloc`); we need object allocations to push past the
#    threshold.
#
# The fix returns a fresh sp_str_alloc'd copy whose own `\xfe` tag
# byte makes mark_string take the right path, and which survives
# independently of the original path.

class Trash
  def initialize(n)
    @n = n
    @s = "padding payload " * 64    # ~1 KB heap string per Trash
  end
  attr_reader :n
end

def name_of(i)
  path = "/very/deep/parent/directory/with/many/segments/file" + i.to_s + ".rb"
  File.basename(path)
end

names = []
i = 0
while i < 50
  names << name_of(i)
  i = i + 1
end

# Allocate ~5000 Trash instances (~5 MB) — far past the 256 KB GC
# threshold — to force multiple sp_gc_collect cycles and reap the
# unreferenced path strings via sp_str_sweep.
junk = []
j = 0
while j < 5000
  junk << Trash.new(j)
  j = j + 1
end
puts junk.length     # 5000

# Each names[i] should still resolve to "fileN.rb".
ok = 1
i = 0
while i < 50
  expected = "file" + i.to_s + ".rb"
  if names[i] != expected
    ok = 0
  end
  i = i + 1
end
puts(ok == 1 ? "ok" : "corrupt")    # ok
