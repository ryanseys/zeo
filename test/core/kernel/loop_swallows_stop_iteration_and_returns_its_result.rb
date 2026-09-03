# `loop`'s full StopIteration contract (CRuby kernel.rb:151): a manual
# `raise StopIteration` returns nil (no result set), `break value` still
# carries its value out, and every OTHER exception propagates.

p(loop { raise StopIteration })
p(loop { break 42 })
begin
  loop { raise ArgumentError, "boom" }
rescue ArgumentError => e
  puts "arg: #{e.message}"
end
__END__
nil
42
arg: boom
