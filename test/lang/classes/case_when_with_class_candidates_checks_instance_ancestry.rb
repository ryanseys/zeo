# `Module#===` -- previously a compile-time rejection (`when Integer`
# never worked); now real Ruby's instance-of check.

class Widget
end

[5, "s", Widget.new, nil].each do |v|
  case v
  when Integer then puts "int"
  when String then puts "str"
  when Widget then puts "widget"
  else puts "other"
  end
end
__END__
int
str
widget
other
