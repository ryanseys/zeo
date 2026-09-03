# Same gate, caught: since the miss surfaces through the runtime
# constant path it's an ordinary rescuable `NameError` (oracle prints
# `rescued: uninitialized constant Base64`). This program NEVER requires
# base64, so activation stays off -- avoiding the documented
# program-global-activation divergence (a `require` anywhere activates
# the constant everywhere, unlike CRuby's file-ordered visibility).

begin
  Base64.strict_encode64("hi")
rescue NameError => e
  puts "rescued: #{e.message}"
end
__END__
rescued: uninitialized constant Base64
