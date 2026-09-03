# One raiser builds every NoMethodError message, so the four receiver
# shapes stay in step: an instance, nil (rendered bare, no "an instance
# of" prefix), a module, and a class. Critically the receiver is described
# by CLASS NAME and never by calling `inspect` on it -- CRuby's formats
# carry no receiver-inspect directive, which is what lets an object with
# no `inspect` still raise an error about itself.

def msg
  yield
rescue NoMethodError => e
  puts e.message
end
module Helper; end
msg { Object.new.nope }
msg { nil.nope }
msg { Helper.nope }
msg { String.nope }
msg { 5.nope }
__END__
undefined method 'nope' for an instance of Object
undefined method 'nope' for nil
undefined method 'nope' for module Helper
undefined method 'nope' for class String
undefined method 'nope' for an instance of Integer
