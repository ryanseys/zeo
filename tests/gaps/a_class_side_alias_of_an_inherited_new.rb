class Q1; end
Q1.singleton_class.class_eval { alias_method :made, :new }
p Q1.made.class.to_s
