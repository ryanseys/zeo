# net/smtp loads its authenticators with a glob-and-require, and rubygems its
# plugins the same way. The pattern is a compile-time fact, so the matches are
# spliced like any other require -- and because they were, the loop's own
# `require_relative` finds them already loaded and answers false rather than
# raising.
require "net/smtp"

p defined?(Net::SMTP::AuthPlain)
p defined?(Net::SMTP::AuthLogin)
p defined?(Net::SMTP::AuthCramMD5)
p Net::SMTP::AuthPlain.superclass

# `$LOADED_FEATURES` holds every file the front end spliced, so re-requiring one
# is the no-op Ruby makes it.
p $LOADED_FEATURES.is_a?(Array)
p $LOADED_FEATURES.any? { |f| f.end_with?("net/smtp.rb") }
p $LOADED_FEATURES.any? { |f| f.end_with?("net/smtp/auth_plain.rb") }
p $" .equal?($LOADED_FEATURES)

# Socket.tcp exists (net/http probes for it to decide which connect path to
# take); no connection is attempted here.
require "socket"
p Socket.respond_to?(:tcp)
p Socket.method(:tcp).is_a?(Method)

# `deprecate_constant` validates its arguments the way its siblings do.
module Legacy
  OLD = 1
  deprecate_constant :OLD
end
p Legacy::OLD
begin
  Legacy.deprecate_constant(:NOPE)
rescue NameError => e
  puts e.message
end
__END__
"constant"
"constant"
"constant"
Net::SMTP::Authenticator
true
true
true
true
true
true
1
constant Legacy::NOPE not defined
