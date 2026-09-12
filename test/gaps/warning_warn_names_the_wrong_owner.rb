# Who owns `Warning.warn`.
p Warning.singleton_methods(false).sort
p Warning.method(:warn).owner.to_s
p Warning.instance_methods(false).sort
__END__
[:[], :[]=, :categories]
"Warning"
[:warn]
