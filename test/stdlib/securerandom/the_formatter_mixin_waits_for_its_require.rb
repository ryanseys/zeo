# Random::Formatter holds only rand and random_number until
# `require "random/formatter"` (which securerandom pulls in as well).
FAMILY = %i[alphanumeric base64 hex random_bytes urlsafe_base64 uuid uuid_v4].freeze
p Random::Formatter.instance_methods(false).sort
p (Random::Formatter.instance_methods(false) & FAMILY).sort
p Random::Formatter.private_instance_methods(false).sort
p Random.new.respond_to?(:hex), Random.new.respond_to?(:random_number)
require "random/formatter"
p (Random::Formatter.instance_methods(false) & FAMILY).sort
p Random::Formatter.private_instance_methods(false).sort
p Random.new.hex(4).length, Random.new.uuid.length
__END__
[:rand, :random_number]
[]
[]
false
true
[:alphanumeric, :base64, :hex, :random_bytes, :urlsafe_base64, :uuid, :uuid_v4]
[:choose, :gen_random]
8
36
