# include, extend and prepend are ordinary calls, so the error comes from the
# class body's own line rather than from the compile.
# (spinel issue #3330)
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
compiler/fusion/an_unresolved_include_target_is_a_runtime_name_error.rb:5:in '<module:Sock>': uninitialized constant Sock::FFI (NameError)
	from compiler/fusion/an_unresolved_include_target_is_a_runtime_name_error.rb:4:in '<main>'
#@ exit 1
