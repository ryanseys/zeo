# The other old silent-drop site: kwargs on a Poly receiver now ride
# the G2 convention; an undefined method stays a real NoMethodError.

begin
  raise "boom"
rescue => e
  begin
    e.foo(bar: 1)
  rescue NoMethodError
    puts "no method, kwargs carried"
  end
end
__END__
no method, kwargs carried
