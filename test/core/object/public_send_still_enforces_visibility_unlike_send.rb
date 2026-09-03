# Same intent as when this asserted a compile-time panic, now asserting
# the CORRECT mechanism: real Ruby resolves visibility at call time
# (`rb_method_call_status`, vm_eval.c:837) and raises a rescuable
# NoMethodError. Rejecting it during codegen was wrong -- it killed any
# program that merely mentions such a call, even in a rescued branch --
# so the program must now compile and the raise must be catchable, while
# `send` stays visibility-blind.

class Box
  private

  def secret
    "shh"
  end
end

begin
  puts Box.new.public_send(:secret)
rescue NoMethodError => e
  puts e.message
end
puts Box.new.send(:secret)
__END__
private method 'secret' called for an instance of Box
shh
