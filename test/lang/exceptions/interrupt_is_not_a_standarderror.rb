# `rescue StandardError` does not catch Interrupt; `rescue Exception` does.
begin
  raise Interrupt
rescue StandardError
  puts "wrong: StandardError caught Interrupt"
rescue Exception => e
  puts "ok: #{e.class}"
end
begin
  raise ArgumentError, "x"
rescue RuntimeError
  puts "wrong: RuntimeError caught ArgumentError"
rescue ArgumentError
  puts "ok: ArgumentError"
end
begin
  raise ArgumentError, "y"
rescue StandardError => e
  puts "ok: StandardError caught #{e.class}"
end
begin
  raise "plain"
rescue RuntimeError => e
  puts "ok: RuntimeError caught #{e.class}"
end
__END__
ok: Interrupt
ok: ArgumentError
ok: StandardError caught ArgumentError
ok: RuntimeError caught RuntimeError
