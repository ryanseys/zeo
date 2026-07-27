# zeo REJECTS this at compile time ("unknown module `FFI`"); ruby compiles it
# and raises NameError from the module body at run time. The divergence is
# WHEN an unresolved constant is reported, not whether it is.
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
