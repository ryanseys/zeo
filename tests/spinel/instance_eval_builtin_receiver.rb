# instance_eval / instance_exec with a block on a BUILTIN receiver (#2634):
# the body is spliced inline with self bound to a temp; receiverless calls
# dispatch on the new self (Kernel globals stay receiverless), and the call's
# value is the block's. User-object receivers keep the dedicated splice that
# handles their ivars and methods.
p "abc".instance_eval { upcase }
p 5.instance_eval { self + 1 }
p [1, 2].instance_eval { length * 10 }
p "xy".instance_exec(3) { |n| self * n }
p "s".instance_eval { |s| s + "!" }
p "abc".instance_eval { self.upcase }
p "hi".instance_eval { puts upcase; length }
x = "abc".instance_eval { 1 + 1 }
p x
# multi-statement body with locals
p("ab".instance_eval do
  u = upcase
  "#{u}/#{length}"
end)
# user-object receivers still ride the ivar-aware path
class W
  def initialize; @v = 7; end
  def get; @v; end
end
p W.new.instance_eval { @v + 1 }
p W.new.instance_eval { get * 2 }
