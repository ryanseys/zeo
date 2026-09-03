# A `require` zeo resolves at COMPILE time is spliced in place, and the
# spliced body has to land INSIDE the `begin` that wrote the require. CRuby
# runs the required file during the `require` call, so a raise from the
# file's top level both reaches the handler around it and aborts the rest of
# the begin's body.
#
# zeo spliced it out as a SIBLING, and broke both halves: the program died
# where ruby rescues, and the statements after the require ran anyway. It was
# hidden because a require zeo CANNOT resolve statically takes the run-time
# path, which was always rescuable.
#
# `begin; require "optional"; rescue LoadError; end` is the everyday shape.

$LOAD_PATH.unshift(File.expand_path("../../fixtures/raising_feature", __dir__))

begin
  require "r1"
  puts "after require -- must not print"
rescue => e
  puts "bare rescue        #{e.class}: #{e.message}"
end

begin
  require "r2"
rescue ArgumentError
  puts "named rescue       ok"
ensure
  puts "ensure             ran"
end

# Nothing raises: the `else` runs and the handler stays dead.
begin
  require_relative "../../fixtures/raising_feature/ok"
  puts "body finished      #{LOADED_CLEANLY}"
rescue ArgumentError
  puts "must not run"
else
  puts "else               ran"
end

begin
  require "r3"
rescue TypeError
  puts "must not run"
rescue ArgumentError
  puts "second clause      ok"
end

begin
  begin
    require "r4"
  rescue ArgumentError
    puts "inner rescue       ok"
  end
rescue Exception
  puts "outer must not run"
end

# A method-body require runs when the method does, not at load.
def load_r5
  require "r5"
rescue ArgumentError => e
  "method rescued: #{e.message}"
end
puts "method body        #{load_r5}"

# The begin is an EXPRESSION: the handler's value is the begin's value.
value = begin
  require "r6"
  "no raise"
rescue ArgumentError
  "value from rescue"
end
puts "as an expression   #{value}"

# `require_relative` and `load` take the same path.
begin
  require_relative "../../fixtures/raising_feature/r7"
rescue ArgumentError => e
  puts "require_relative   #{e.message}"
end

begin
  load File.expand_path("../../fixtures/raising_feature/r8.rb", __dir__)
rescue ArgumentError => e
  puts "load               #{e.message}"
end

puts "still running"
__END__
bare rescue        ArgumentError: raise from r1
named rescue       ok
ensure             ran
body finished      true
else               ran
second clause      ok
inner rescue       ok
method body        method rescued: raise from r5
as an expression   value from rescue
require_relative   raise from r7
load               raise from r8
still running
