# When a regex sub / gsub / scrub result grows past the initial buffer
# capacity, the buffer must be grown on a real malloc base. Previously
# these built into a sp_str_alloc'd buffer and grew it with realloc on a
# pointer offset past the string header -- undefined behaviour that
# corrupted the heap once the output exceeded the initial slack. Each
# input below produces output larger than its starting capacity.

# gsub: 50 'a' -> 50 * "zzzz" = 200 bytes (cap starts ~180).
g = ("a" * 50).gsub(/a/, "zzzz")
p g.length
p g[0, 8]

# sub with a backreference that repeats the whole match: 100 -> 200.
s = ("x" * 100).sub(/x+/, '\0\0')
p s.length

# gsub(regex, hash) with replacement longer than the match.
h = { "a" => "zzzzz" }
gh = ("a" * 40).gsub(/a/, h)
p gh.length

# scrub: 100 invalid bytes, each replaced by a 5-char string -> 500.
sc = ("\xFF" * 100).scrub("XXXXX")
p sc.length
p sc == "X" * 500
