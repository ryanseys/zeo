class Base2
  LIMIT = 5
end

class Sub2 < Base2
  def l
    LIMIT
  end
end

puts Sub2.new.l
__END__
5
