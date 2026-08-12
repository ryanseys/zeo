# `require_relative` of a NATIVE (.so/.bundle) feature names a compiled
# extension zeo cannot load. The call keeps its site and raises the catchable
# runtime LoadError -- CRuby's own answer when the file is absent -- instead
# of failing the whole compile. Both the rescued and the bare-but-reached
# spellings stay programs.
begin
  require_relative "ext/never_built.so"
rescue LoadError => e
  p e.class
  p e.message.include?("never_built")
end

def optional_native
  require_relative "also_never_built.bundle"
rescue LoadError
  :fell_back
end
p optional_native
puts "still running"
