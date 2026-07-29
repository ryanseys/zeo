# Kernel's module functions answer to the module as an explicit receiver, and
# mean exactly what the bare call means. `Kernel.exit` is how a CLI library
# makes its exit path injectable (`lambda { |s| Kernel.exit(s) }`), and
# `Kernel.puts` is how one prints from a class that defines its own `puts`.
#
# The frame-sensitive ones are the interesting half: Ruby's module-function
# copy reads the CALLER's frame, so `Kernel.block_given?` and `Kernel.binding`
# ask about the enclosing method exactly as the bare spellings do.

Kernel.puts "puts ok"
Kernel.print "print ok\n"
p Kernel.format("%05.2f", 3.5)
p Kernel.sprintf("%s-%s", "a", "b")
p Kernel.Integer("42")
p Kernel.Integer("ff", 16)
p Kernel.Float("1.5")
p Kernel.String(9)
p Kernel.Array([1, 2])
p Kernel.rand(1)
p Kernel.srand(7).class
p Kernel.lambda { |x| x }.lambda?
p Kernel.proc { |x| x }.lambda?
p Kernel.catch(:done) { Kernel.throw(:done, 5) }
p Kernel.binding.class
p Kernel.caller.class

begin
  Kernel.raise ArgumentError, "boom"
rescue ArgumentError => e
  p e.message
end

# --- the frame-sensitive ones ask about the ENCLOSING method ----------------
def probe
  [block_given?, Kernel.block_given?, Kernel.iterator?]
end
p probe
p probe { }

def named
  [__method__, Kernel.__method__, Kernel.__callee__]
end
p named

def locals
  apple = 1
  banana = 2
  [apple, banana] # keep both live
  Kernel.local_variables.sort
end
p locals

# --- file tests -------------------------------------------------------------
p Kernel.test(?e, "/")
p Kernel.test(?d, "/")
p Kernel.test(?f, "/")
p Kernel.test(?e, "/no/such/path/at/all")
begin
  Kernel.test(??, "/")
rescue ArgumentError => e
  p e.class
end

# --- globals ----------------------------------------------------------------
$zeo_probe_global = 1
p Kernel.global_variables.include?(:$zeo_probe_global)
p global_variables.include?(:$zeo_probe_global)
p Kernel.global_variables.include?(:$no_such_global_anywhere)

# --- exit, through a lambda, which is how it shows up in real code ----------
bye = lambda { |status| Kernel.exit(status) }
p bye.class.to_s
begin
  bye.call(3)
rescue SystemExit => e
  p e.status
end

p Kernel.select([], [], [], 0)
Kernel.exit(0)
Kernel.puts "not reached"
