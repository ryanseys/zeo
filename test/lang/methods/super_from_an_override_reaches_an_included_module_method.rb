module Tagged
  def tag
    "base(#{super})"
  end
end
class Widget
  include Tagged
  def tag
    "widget"
  end
end
puts Widget.new.tag
__END__
widget
