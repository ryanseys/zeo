# `raise <computed>, message` -- a runtime-computed exception class/instance
# (not a literal constant) coerces like Kernel#raise (`klass.exception(msg)`).
def boom(klass) = raise klass, "boom from #{klass}"

[ArgumentError, TypeError, RuntimeError].each do |k|
  begin
    boom(k)
  rescue => e
    puts "#{e.class}: #{e.message}"
  end
end
