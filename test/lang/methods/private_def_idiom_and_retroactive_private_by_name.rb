class A
  def pub_a
    priv_a
  end

  private def priv_a
    "priv_a"
  end
end

class B
  def pub_b
    priv_b
  end

  def priv_b
    "priv_b"
  end

  private :priv_b
end

puts A.new.pub_a
puts B.new.pub_b
__END__
priv_a
priv_b
