# A user subclass of a builtin carries the builtin's native object as a
# payload. `StringIO` and `File` join Array/String/Hash/StringScanner as
# payload roots: puma's `IOBuffer < StringIO` and aws-sdk's `ManagedFile <
# File` are between them the whole aws-sdk-* family's blocker.
#
# `File` is the first root whose own methods are mostly INHERITED -- `read`
# lives on `IO` -- so the payload bridge follows the root's builtin
# superclasses, and stops before Object (`Buffer.new.class` is `Buffer`, not
# `StringIO`).

require "stringio"
require "tmpdir"

class Buffer < StringIO
  def dump = "<#{string}>"
end

b = Buffer.new("hello")
p b.dump
p b.read
p [b.is_a?(StringIO), b.class, Buffer.superclass]

class Managed < File
  def open? = !closed?
end

path = File.join(Dir.tmpdir, "zeo_value_subclass_example.txt")
File.write(path, "data")
f = Managed.new(path)
p [f.open?, f.read, f.class]
f.close
p f.open?
File.unlink(path)
