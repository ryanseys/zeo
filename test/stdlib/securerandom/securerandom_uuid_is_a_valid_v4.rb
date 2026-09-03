require "securerandom"
u = SecureRandom.uuid
puts(u =~ /\A\h{8}-\h{4}-4\h{3}-[89ab]\h{3}-\h{12}\z/ ? "uuid_ok" : "uuid_BAD: #{u}")
puts(SecureRandom.uuid_v4 =~ /\A\h{8}-\h{4}-4\h{3}-[89ab]\h{3}-\h{12}\z/ ? "v4_ok" : "v4_BAD")
__END__
uuid_ok
v4_ok
