# `catch` with no block raises LocalJumpError, in every spelling: with a tag,
# bare, through `send`, and with an explicit nil block-pass. The catch frame
# must not be left standing, so a later `throw` of the same tag is an
# UncaughtThrowError rather than a match.
[
  -> { catch(:tag) },
  -> { catch },
  -> { Kernel.send(:catch, :tag) },
  -> { catch(:tag, &nil) },
].each do |probe|
  probe.call
rescue LocalJumpError => e
  puts "#{e.class}: #{e.message}"
end

begin
  throw :tag
rescue UncaughtThrowError => e
  puts "#{e.class}: #{e.message}"
end

puts catch(:tag) { |t| throw t, "through" }
__END__
LocalJumpError: no block given
LocalJumpError: no block given
LocalJumpError: no block given
LocalJumpError: no block given
UncaughtThrowError: uncaught throw :tag
through
