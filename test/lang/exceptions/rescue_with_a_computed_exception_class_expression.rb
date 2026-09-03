# A rescue-list entry that is a COMPUTED expression (not a bare constant or
# splat) is evaluated and matched at runtime -- net/http's `rescue
# defined?(OpenSSL::SSL) ? OpenSSL::SSL::SSLError : IOError`.

class MyErr < StandardError; end
def go
  raise MyErr, "boom"
rescue IOError, (defined?(Nope) ? Nope : MyErr) => e
  "caught #{e.class}: #{e.send(:message)}"
end
puts go
__END__
caught MyErr: boom
