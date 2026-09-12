# A method a reopen ADDS to a builtin arrives at the `def`, not at boot.
p Dir.respond_to?(:scratch), String.method_defined?(:shout)
class Dir
  def self.scratch = "/tmp"
end
class String
  def shout = upcase + "!"
end
p Dir.respond_to?(:scratch), String.method_defined?(:shout)
p Dir.scratch, "hi".shout
__END__
false
false
true
true
"/tmp"
"HI!"
