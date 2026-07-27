# Kernel's module functions are callable with the module as an explicit
# receiver and mean exactly what the bare call means. Nothing served that
# receiver, so every one raised "undefined method 'exit' for class Kernel" --
# including the `lambda { |status| Kernel.exit(status) }` idiom a CLI library
# uses to make its exit path injectable.
Kernel.puts "puts ok"
Kernel.print "print ok\n"
p Kernel.format("%05.2f", 3.5)
p Kernel.sprintf("%s-%s", "a", "b")
p Kernel.Integer("42")
p Kernel.Integer("ff", 16)
p Kernel.Float("1.5")
p Kernel.String(9)
p Kernel.Array([1, 2])
p Kernel.block_given?
p Kernel.rand(1)

begin
  Kernel.raise ArgumentError, "boom"
rescue ArgumentError => e
  p e.message
end

# through a lambda, which is how it shows up in real code
bye = lambda { |status| Kernel.exit(status) }
p bye.class.to_s

at_exit_ran = false
p at_exit_ran

Kernel.exit(0)
Kernel.puts "not reached"
