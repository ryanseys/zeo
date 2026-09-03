def show(label)
  puts format("%-30s %s", label, (yield).inspect)
rescue => e
  puts format("%-30s %s", label, e.class)
end

def plain = "plain"
show("plain def is private")   { Object.private_method_defined?(:plain) }

public
def after_public = "after_public"
show("after public is public") { Object.public_method_defined?(:after_public) }
show("after public not private") { Object.private_method_defined?(:after_public) }
show("callable through self")  { self.after_public }

private
def after_private = "after_private"
show("after private")          { Object.private_method_defined?(:after_private) }

public
def second_public = "second"
show("second public run")      { Object.public_method_defined?(:second_public) }

show("method owner")           { method(:private).owner.to_s.start_with?("#<Class:") }
__END__
plain def is private           true
after public is public         true
after public not private       false
callable through self          "after_public"
after private                  true
second public run              true
method owner                   true
