# `def Klass.name` written inside `class Klass` is the older spelling of
# `def self.name` -- net/smtp uses it throughout, including as the target of a
# `class << self` alias, which only resolves if it registered as a class method.
module Net
  class SMTP
    def SMTP.default_port = 25
    def SMTP.default_tls_port = 465

    class << self
      alias default_ssl_port default_tls_port
    end

    def SMTP.start(address, port = nil)
      "start #{address}:#{port || default_port}"
    end
  end
end

p Net::SMTP.default_port
p Net::SMTP.default_ssl_port
p Net::SMTP.start("mail.example")
p Net::SMTP.singleton_methods.sort
p Net::SMTP.new.respond_to?(:default_port)

# A DIFFERENT constant is a genuine per-object singleton def, not a class
# method of the body being lowered.
class Host
  Target = Object.new
  def Target.ping = "pong"
end
p Host::Target.ping
p Host.singleton_methods
__END__
25
465
"start mail.example:25"
[:default_port, :default_ssl_port, :default_tls_port, :start]
false
"pong"
[]
