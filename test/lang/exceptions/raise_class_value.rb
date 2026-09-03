# `raise` given the exception CLASS as a VALUE -- a local, an element of a
# table of error classes, a retry list -- rather than as a syntactically
# literal constant name. Codegen builds the literal form directly and hands
# every other operand to the runtime coercion, which raises a fresh instance
# of an Exception class through `#exception` (so a custom `initialize` runs),
# raises an Exception object as itself, wraps a String in a `RuntimeError`,
# and answers `TypeError: exception class/object expected` for anything else.
module App
  class Failed < RuntimeError; end
  class Refused < App::Failed; end
end

k = App::Failed
begin
  raise k
rescue => e
  puts "#{e.class}|#{e.message}|#{e.is_a?(App::Failed)}"
end

[App::Failed, App::Refused, ArgumentError].each do |kk|
  begin
    raise kk
  rescue => e
    puts "#{e.class}|#{e.is_a?(App::Failed)}"
  end
end

# a rescue arm still selects on the raised class
begin
  raise App::Refused
rescue App::Failed => e
  puts "caught #{e.class}"
end

# a non-exception class is still CRuby's TypeError
bad = String
begin
  raise bad
rescue TypeError => e
  puts "TypeError"
end
__END__
App::Failed|App::Failed|true
App::Failed|true
App::Refused|true
ArgumentError|false
caught App::Refused
TypeError
