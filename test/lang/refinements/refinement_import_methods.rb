# Refinement#import_methods: copies a module's OWN methods into the
# refinement at their declared visibility; ancestors are not imported.
module Helpers
  def shout = "#{self}!"
  private def whisper = "(#{self})"
end

module Loud
  refine String do
    import_methods Helpers
  end
end

# Private on Module, private on Refinement.
p Module.private_instance_methods(false).include?(:refine)
p Module.private_instance_methods(false).include?(:using)
p Refinement.private_instance_methods(false).include?(:import_methods)

ref = Loud.refinements.first
p ref.class
p ref.instance_methods(false).sort
p ref.private_instance_methods(false).include?(:whisper)

# Argument validation happens before any import.
module Bad
  refine String do
    begin
      import_methods Helpers, "nope"
    rescue TypeError => e
      puts e.message
    end
  end
end
__END__
true
true
true
Refinement
[:shout]
true
wrong argument type String (expected Module)
