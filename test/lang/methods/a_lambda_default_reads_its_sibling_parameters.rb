# A lambda written as a parameter DEFAULT closes over the parameters bound
# before it: the default is evaluated in the callee's frame, so `dest` and
# `out` are that frame's locals and the lambda captures them like any block
# in the body would. zeo walked the body alone for escaping blocks, so the
# parameters stayed plain slots and the lambda's read of `dest` had nothing
# to read.

def home(dest: "d", out: $stdout, build: -> { vendor(dest: dest, out: out) })
  build.call
end
def vendor(dest:, out:) = out.puts("vendor #{dest}")
home
home(dest: "x")

def opt(a = 1, f = -> { a * 2 }) = f.call
p opt
p opt(5)
p opt(5, -> { :given })

def late(a = 1, f = proc { a += 1; a }, b = f.call)
  [a, b, f.call]
end
p late
__END__
vendor d
vendor x
2
10
:given
[2, 2, 3]
