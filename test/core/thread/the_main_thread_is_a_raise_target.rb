# A sibling raising into MAIN: delivered at main's next checkpoint --
# here, waking its sleep -- and rescuable like any other raise.

main = Thread.main
t = Thread.new { sleep 0.05; main.raise("to-main") }
begin
  sleep 5
  puts "overslept"
rescue => e
  puts "main caught: #{e.message}"
end
t.join
__END__
main caught: to-main
