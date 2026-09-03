# `include`/`extend`/`prepend` are ordinary method calls, so an unresolved
# target is a RUNTIME NameError raised from the class body -- on the directive's
# own line, with the enclosing bodies below it -- not a compile-time rejection.
module Sock
  extend FFI
  ffi_lib "c"
  attach_function :puts_c, :puts, [:str], :int
end

class Boot
  def start
    if Missing::CONF.cert.length > 0
      Sock.puts_c(Missing::CONF.cert)
    end
    deadline = Time.now.to_i + timeout_seconds
    deadline
  end
end

puts "ok"
__END__
#@ stderr
compiler/fusion/issue_3330_unresolved_at_typed_slots.rb:5:in '<module:Sock>': uninitialized constant Sock::FFI (NameError)
	from compiler/fusion/issue_3330_unresolved_at_typed_slots.rb:4:in '<main>'
#@ exit 1
