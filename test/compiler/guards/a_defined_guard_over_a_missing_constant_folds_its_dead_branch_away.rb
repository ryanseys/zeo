# The dead arm references `Absent::Thing` and calls a method that doesn't
# exist on `Shim` -- both must be eliminated, not emitted, or the program
# fails to compile. CRuby's reachability agrees the branch never runs.

class Shim; end
v = defined?(Absent::Thing) ? Absent::Thing : "fallback"
p v
def guard
  if defined?(NoSuchFeature) && NoSuchFeature.on?
    Shim.new.method_that_does_not_exist
    "on"
  else
    "off"
  end
end
p guard
p defined?(Absent::Thing)
p defined?(String)
__END__
"fallback"
"off"
nil
"constant"
