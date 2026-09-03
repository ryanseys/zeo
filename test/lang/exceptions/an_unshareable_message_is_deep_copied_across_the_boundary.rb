# Mutating the original AFTER send must not affect the ractor's copy --
# the whole point of the isolation discipline.
# Asserted via `puts` on the round-tripped copy (collection methods on
# the Poly-typed receive result are the pre-existing Poly-dispatch
# gap, nothing Ractor-specific).

echo = Ractor.new do
  Ractor.receive
end
payload = [100, 200]
echo.send(payload)
payload[0] = 999
puts echo.value
__END__
100
200
#@ stderr
lang/exceptions/an_unshareable_message_is_deep_copied_across_the_boundary.rb:7: warning: Ractor API is experimental and may change in future versions of Ruby.
