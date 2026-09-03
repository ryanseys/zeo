class Always
  def ==(o) = true
  def inspect = "#<Always>"
end

a = [Always.new]
p a.delete(1)
p a
p [1, 2].delete(1)
p [1, 2].delete(9)
p [1, 2].delete(9) { :blk }
__END__
#<Always>
[]
1
nil
:blk
