h = {}
p h.public_send(:default_proc=, :a).class
p h.default_proc.class
__END__
Proc
Proc
