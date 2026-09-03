# ENV's error shapes: a non-String key is a TypeError, a bare `fetch` miss
# is a KeyError.

begin
  ENV[:PATH]
rescue TypeError => e
  puts "TypeError"
end
begin
  ENV.fetch("ZEO_DEFINITELY_ABSENT")
rescue KeyError => e
  puts "KeyError"
end
__END__
TypeError
KeyError
