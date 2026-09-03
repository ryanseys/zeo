# A feature with no Ruby half falls through to the static-ext table, which is
# the ordinary case for most extensions.

require "base64"
p Base64.encode64("hi")
__END__
"aGk=\n"
