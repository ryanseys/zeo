require "timeout"

puts Timeout.timeout(5) { "finished in time" }

begin
  Timeout.timeout(0.01) { sleep 5 }
rescue Timeout::Error => e
  puts "timed out: #{e.class}"
end

begin
  Timeout.timeout(0.01, ArgumentError) { sleep 5 }
rescue ArgumentError => e
  puts "custom class: #{e.class}"
end

begin
  Timeout.timeout(0.01, Timeout::Error, "slow thing") { sleep 5 }
rescue Timeout::Error => e
  puts "message: #{e.message}"
end

# A nil or non-positive limit never arms the timer.
puts Timeout.timeout(nil) { "no limit" }
puts Timeout::Error.ancestors.include?(RuntimeError)
