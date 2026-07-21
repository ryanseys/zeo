# defined? on globals and methods. Methods already returned
# "method"; globals were returning "expression". CRuby returns
# "global-variable" for assigned $x but nil for never-assigned
# globals (the name alone is not "defined" if never written).
$g = 1
puts defined?($g)
puts defined?($unset_gvar)

def foo; end
puts defined?(foo)

# Locals already correct.
x = 1
puts defined?(x)
puts defined?(undefined_local).nil?
