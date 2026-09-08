# A `require_relative` written inside a `def` runs when the METHOD runs, not
# where the file that defines it is loaded. So the target's constants are
# absent until the first call, the first call answers true and the second
# false, and a program that never calls the method never loads the file.
def pull = require_relative("a_require_relative_in_a_method_runs_when_the_method_does/counted")

p defined?(COUNTED_RUNS)
p pull
p pull
p COUNTED_RUNS

def never_called = require_relative("a_require_relative_in_a_method_runs_when_the_method_does/late")

p defined?(Late)
p never_called
p Late.name_of
__END__
nil
counted ran
true
false
1
nil
late ran
true
"late"
