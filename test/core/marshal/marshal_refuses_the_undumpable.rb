# `Marshal.dump` refuses what cannot round-trip: an object with a singleton
# ("singleton can't be dumped") and an IO ("can't dump IO"). zeo dumps both
# happily, silently losing the singleton and the stream -- wrong DATA, not
# just a missing error. (Found by the 2026-08-24 probe sweep.)
o = Object.new
def o.only_mine = 1
begin
  Marshal.dump(o)
  puts "singleton dumped"
rescue TypeError => e
  puts e.message
end
begin
  Marshal.dump($stdout)
  puts "io dumped"
rescue TypeError => e
  puts e.message
end
__END__
singleton can't be dumped
can't dump IO
