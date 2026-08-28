# `Marshal` carries the instance variables set on a container, not only its
# elements: an Array with `@meta` comes back with `@meta`. zeo drops them,
# so the reloaded array answers nil.
#
# Carved out of tests/probe_roundtrip.rb ("marshal array ivar").
a = [1]
a.instance_variable_set(:@meta, 7)
back = Marshal.load(Marshal.dump(a))
p back
p back.instance_variable_get(:@meta)
