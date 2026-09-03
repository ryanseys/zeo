# Assert the UNCAUGHT path here (see the `rescue` tests below for the
# caught path): an unhandled `raise MyError` (no explicit message) exits 1 with the
# class's own name as the message (no runtime `self.class` reflection
# needed -- see the emitter's raise lowering in `clif::stmt`).

class MyError < StandardError
end
class Box
  def check
    raise MyError
  end
end
Box.new.check
__END__
#@ stderr
lang/exceptions/raise_with_a_bare_class_defaults_the_message_to_the_class_name.rb:10:in 'Box#check': MyError (MyError)
	from lang/exceptions/raise_with_a_bare_class_defaults_the_message_to_the_class_name.rb:13:in '<main>'
#@ exit 1
