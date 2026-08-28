a = [1]
a.instance_variable_set(:@meta, 7)
back = Marshal.load(Marshal.dump(a))
p back
p back.instance_variable_get(:@meta)
