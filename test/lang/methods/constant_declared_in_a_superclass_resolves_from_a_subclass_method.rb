class Base
  X = 1
end
class Sub < Base
  def get
    X
  end
end
puts Sub.new.get
__END__
1
