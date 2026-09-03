# defined?(@iv) is "instance-variable" only once assigned, else nil.
# (A never-assigned ivar referenced ONLY inside a class body's defined?
# hits the documented assigned-nil-vs-never-assigned limitation for
# struct-backed objects; top-level ivars use a dynamic map and are exact.)

p defined?(@never)
@written = 1
p defined?(@written)
class C
  def initialize; @a = 1; end
  def check; defined?(@a); end
end
p C.new.check
__END__
nil
"instance-variable"
"instance-variable"
