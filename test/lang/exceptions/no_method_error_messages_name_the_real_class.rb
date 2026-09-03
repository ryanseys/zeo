class Widget
end

w = [Widget.new].first
begin
  w.nope
rescue NoMethodError => e
  puts e.message
end

module Helper
end

begin
  Helper.new
rescue NoMethodError => e
  puts e.message
end
__END__
undefined method 'nope' for an instance of Widget
undefined method 'new' for module Helper
