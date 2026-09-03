# A `def` nested in a `case`/`if` branch (a platform-conditional definition,
# as in fileutils' StreamUtils_) is registered as an own method, so
# `instance_methods` and `extend` -- both resolved at compile time in zeo --
# can see it. A branch the compiler folds away (`if false`) still defines
# nothing, matching CRuby.

module Inner
  case 1
  when 2 then def flavor; "two"; end
  else def flavor; "other"; end
  end
  if false
    def gated; "on"; end
  end
end
p Inner.instance_methods(false).sort
module Host
  extend Inner
  puts flavor
end
p Host.respond_to?(:gated)
__END__
[:flavor]
other
false
