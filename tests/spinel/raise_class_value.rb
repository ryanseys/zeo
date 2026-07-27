# `raise SomeError` was only recognized as raising SomeError when the class was
# written as a literal constant. Reached through a variable or an array element
# -- a table of error classes, a retry list -- the Class value fell through to
# "exception class/object expected" and raised TypeError instead.
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
