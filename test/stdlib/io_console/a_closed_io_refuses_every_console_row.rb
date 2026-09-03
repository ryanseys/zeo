# Every `io/console` row runs `GetOpenFile` first, so a CLOSED stream is
# `IOError: closed stream` and no syscall happens.
#
# `#fileno` answered 0 for a closed handle, so the whole family ran its
# ioctls and termios calls against descriptor 0 -- STDIN. A closed IO's
# `raw!` put the user's own terminal into raw mode and reported ENOTTY.

require "io/console"

r, w = IO.pipe
r.close
w.close

%i[fileno to_i tty? isatty winsize echo? ttyname iflush oflush cursor
   console_mode].each do |name|
  begin
    r.public_send(name)
    puts "#{name} answered"
  rescue IOError => e
    puts "#{name} #{e.class}: #{e.message}"
  end
end

begin
  r.raw { 1 }
rescue IOError => e
  puts "raw #{e.class}: #{e.message}"
end

begin
  r.winsize = [24, 80]
rescue IOError => e
  puts "winsize= #{e.class}: #{e.message}"
end

# `closed?` itself still answers, as it must.
p r.closed?
__END__
fileno IOError: closed stream
to_i IOError: closed stream
tty? IOError: closed stream
isatty IOError: closed stream
winsize IOError: closed stream
echo? IOError: closed stream
ttyname IOError: closed stream
iflush IOError: closed stream
oflush IOError: closed stream
cursor IOError: closed stream
console_mode IOError: closed stream
raw IOError: closed stream
winsize= IOError: closed stream
true
