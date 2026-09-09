# `trap`'s unsupported-signal ArgumentError uses straight quotes in ruby 4
# ("unsupported signal 'SIGNOPE'"); zeo still uses the old backtick form.
begin
  trap("NOPE") { }
rescue ArgumentError => e
  puts e.message
end
__END__
unsupported signal 'SIGNOPE'
