# A `String#+` loop allocates strings and nothing else. A collector that only
# runs on object allocation never runs here, so the strings pile up
# (sp_gc_bytes). So a string-only workload never collected and RSS grew without
# bound. The fix drives collection off the string heap's own live-byte count.
#
# Assert that a collection actually fires here: GC.stat["cycle"] counts
# sp_gc_collect runs, and since this loop never touches the object heap, the
# ONLY thing that can advance it is the string-heap trigger. Pre-fix this prints
# "NO COLLECTION"; post-fix it prints "collected".
before = GC.stat["cycle"]
s = ""
i = 0
while i < 50000
  s = s + "x"          # fresh result string each step; old s becomes garbage
  s = "" if s.length > 1000   # keep the live set tiny -- all the churn is collectable
  i += 1
end
after = GC.stat["cycle"]
puts(after > before ? "collected" : "NO COLLECTION")
__END__
#@ stderr
core/string/string_plus_heap_gc.rb:19:in '<main>': undefined method '>' for nil (NoMethodError)
#@ exit 1
