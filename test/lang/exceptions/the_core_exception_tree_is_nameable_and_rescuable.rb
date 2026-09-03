# The `Exception`-direct classes (uncaught by a bare `rescue`) and the
# refined `StandardError`-branch classes all register with the right
# superclass and can be raised/rescued by name.

p SystemExit.superclass
p Interrupt.superclass
p NoMemoryError.superclass
p NoMatchingPatternKeyError.superclass
p Regexp::TimeoutError.superclass
p IO::TimeoutError.superclass
begin
  raise SecurityError, "denied"
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
__END__
Exception
SignalException
Exception
NoMatchingPatternError
RegexpError
IOError
SecurityError: denied
