# `Digest(name)` -- the function-shaped algorithm lookup CRuby defines in
# `digest.rb` as a private method of `Object`. It is not a nicety: `pstore`
# picks its checksum with `%w[SHA512 ... MD5].each { break Digest(algo)
# rescue LoadError }`, so both the answer and the LoadError are load-bearing.
#
# zeo had no such row at all, and `require "pstore"` died in the class body.

require "digest"

p Digest(:SHA512)
p Digest(:SHA256)
p Digest(:SHA1)
p Digest(:MD5)
p Digest("SHA384")
p Digest(:SHA512).hexdigest("zeo")

# The `rescue LoadError` pstore leans on -- a name with no class behind it.
begin
  Digest(:NOPE)
rescue LoadError => e
  puts e.message
end

# It answers a class object, so the constant and the call agree.
p Digest(:SHA256).equal?(Digest::SHA256)

# `name.to_sym` is what CRuby writes, so a non-name is a NoMethodError.
begin
  Digest(42)
rescue NoMethodError => e
  puts e.message
end

# Private, exactly as ruby has it: no explicit receiver.
begin
  Object.new.Digest(:MD5)
rescue NoMethodError => e
  puts e.message.split(" for ").first
end
__END__
Digest::SHA512
Digest::SHA256
Digest::SHA1
Digest::MD5
Digest::SHA384
"95a0220cf5fa53d9f9d51559e034f4e75bc34653b5af0fd8517881e95d86d3a7b7c759a825abff954ef8f787c10aaf24d46dc6e6ac22c5395c782296b7518eec"
library not found for class Digest::NOPE -- digest/nope
true
undefined method 'to_sym' for an instance of Integer
private method 'Digest' called
