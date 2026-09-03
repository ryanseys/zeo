class Classifier
  def classify(v)
    case v
    in Integer => n if n % 2 == 0
      "even int #{n}"
    in Integer
      "odd int"
    in String
      "string"
    else
      "other"
    end
  end
end
c = Classifier.new
puts c.classify(4)
puts c.classify(3)
puts c.classify("hi")
puts c.classify(nil)
__END__
even int 4
odd int
string
other
