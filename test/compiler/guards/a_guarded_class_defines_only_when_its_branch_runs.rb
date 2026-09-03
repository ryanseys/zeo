puts defined?(Widget).inspect
def probe? = false
if probe?
  class Widget
    def kind = "guarded"
  end
end
puts defined?(Widget).inspect
begin
  Widget.new
rescue NameError => e
  puts e.class
  puts e.message[0, 30]
end
__END__
nil
nil
NameError
uninitialized constant Widget
