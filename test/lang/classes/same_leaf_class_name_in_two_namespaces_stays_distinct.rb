module A1
  class Widget
    def tag
      "a"
    end
  end
end

module B1
  class Widget
    def tag
      "b"
    end
  end
end

puts A1::Widget.new.tag
puts B1::Widget.new.tag
__END__
a
b
