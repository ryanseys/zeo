# A method call answers "method" only if the receiver responds; a bare
# undefined name is nil. Assignments answer "assignment". nil/true/false
# answer their own name.

def foo; end
p defined?(foo)
p defined?(undefined_zzz)
p defined?(1 + 2)
p defined?(1.nope_zzz)
p defined?(x = 2)
p defined?(@iv = 3)
p defined?(nil)
p defined?(true)
p defined?(false)
p defined?(puts)
__END__
"method"
nil
"method"
nil
"assignment"
"assignment"
"nil"
"true"
"false"
"method"
