# `trap`'s unsupported-signal ArgumentError uses straight quotes in ruby 4
# ("unsupported signal 'SIGNOPE'"); zeo still uses the old backtick form.
# (Found by the 2026-08-24 probe sweep.)
begin
  trap("NOPE") { }
rescue ArgumentError => e
  puts e.message
end
