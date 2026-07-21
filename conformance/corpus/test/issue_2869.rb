# Random.urandom draws from a persistent advancing stream (re-seeding per call
# gave the same byte within a second, so a zero byte never appeared).
p (0...50).map { Random.urandom(1).bytes[0] }.uniq.size > 1
p Random.urandom(4).bytesize
