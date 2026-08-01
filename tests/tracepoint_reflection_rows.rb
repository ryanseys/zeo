# The `TracePoint` rows that read nothing off the frame: the reentry guard,
# the per-VM statistics, and the four accessors a `:line` event refuses in
# CRuby too.

# Outside a handler there is nothing to re-enter.
begin
  TracePoint.allow_reentry { 1 }
rescue RuntimeError => e
  puts "allow_reentry outside: #{e.message}"
end

puts "stat: #{TracePoint.stat.class}"

seen = []
tp = TracePoint.new(:line) do |t|
  %i[return_value parameters eval_script instruction_sequence].each do |m|
    begin
      t.public_send(m)
      seen << "#{m}: answered"
    rescue RuntimeError => e
      seen << "#{m}: #{e.message}"
    end
  end
  t.disable
end

tp.enable
x = 1
tp.disable
puts seen.uniq.join("\n")

# Every accessor refuses from outside a handler first, whatever the event.
%i[return_value parameters self binding].each do |m|
  TracePoint.new(:line) { }.public_send(m)
rescue RuntimeError => e
  puts "#{m} outside: #{e.message}"
end
