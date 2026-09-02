require "io/console"

r, w = IO.pipe
begin
  r.winsize
rescue SystemCallError => e
  puts e.class
end
puts IO.respond_to?(:console), $stdout.respond_to?(:raw), $stdin.respond_to?(:getch)
puts w.respond_to?(:winsize=), r.respond_to?(:noecho)
