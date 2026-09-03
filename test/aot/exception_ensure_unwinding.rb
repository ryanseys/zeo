# Unwinding across ensure blocks and retries: the binary carries its own
# unwinder, not the host ruby's.
def risky(attempts)
  tries = 0
  begin
    tries += 1
    raise IOError, "flaky" if tries < attempts
    "ok after #{tries}"
  rescue IOError
    retry if tries < attempts
    raise
  ensure
    puts "ensure ran on try #{tries}"
  end
end

puts risky(3)
begin
  raise TypeError, "outer"
rescue StandardError => e
  puts "#{e.class}: #{e.message}"
ensure
  puts "outer ensure"
end
__END__
ensure ran on try 3
ok after 3
TypeError: outer
outer ensure
