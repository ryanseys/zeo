class Base
  def self.bump
    1
  end
end
class Sub < Base
end
puts Sub.bump
__END__
1
