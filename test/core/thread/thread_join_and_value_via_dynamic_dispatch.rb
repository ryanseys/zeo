# The threads live in an Array, so `join`/`value` dispatch dynamically
# (the runtime Thread table), not the static codegen fast path.

threads = 3.times.map { |i| Thread.new { i * 10 } }
threads.each(&:join)
p threads.map(&:value)
__END__
[0, 10, 20]
