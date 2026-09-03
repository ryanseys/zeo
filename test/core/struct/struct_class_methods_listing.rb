S = Struct.new(:a, :b)
p S.methods.include?(:members)
p S.methods.include?(:keyword_init?)
p S.members
p S.keyword_init?
__END__
true
true
[:a, :b]
nil
