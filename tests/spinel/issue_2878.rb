# Chaining #drop / #reject onto an each_with_index Enumerator: materialize the
# pairs and apply the array operation.
a = [1.0, 2.0, 3.0]
p a.each_with_index.drop(1).map { |c, i| c * i }
p a.each_with_index.drop(1)
p a.each_with_index.reject { |c, i| i == 0 }
p a.each_with_index.select { |c, i| i > 0 }
