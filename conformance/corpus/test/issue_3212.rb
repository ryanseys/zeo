def maybe_string(flag)
  flag ? " " : nil
end
module Crash
  def self.call(value)
    value.split(/ /)
  end
end
raise unless Crash.call(maybe_string(true)) == []
puts "ok"
