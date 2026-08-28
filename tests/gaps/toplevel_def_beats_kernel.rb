# A top-level `def` lands on Object, which sits ABOVE Kernel in the ancestry, so
# it shadows a Kernel builtin of the same name for a bare call. Spinel reached
# the Kernel arms by position -- they sat above the user-method resolution in
# both halves of the compiler -- so the builtin answered, and analyze and
# codegen could even pick differently: `x = loop(1)` declared x as an
# Enumerator (Kernel#loop) and assigned it the user method's String.
def raise(v) = "raise:#{v}"
def system(v) = "system:#{v}"
def caller(v) = "caller:#{v}"
def puts(v) = "puts:#{v}"
def print(v) = "print:#{v}"
def p(v) = "p:#{v}"
def require(v) = "require:#{v}"
def loop(v) = "loop:#{v}"
def sleep(v) = "sleep:#{v}"
def exit(v) = "exit:#{v}"
def gets(v) = "gets:#{v}"
def sprintf(v) = "sprintf:#{v}"
def format(v) = "format:#{v}"
def rand(v) = "rand:#{v}"

out = []
out << raise(1)
out << system(2)
out << caller(3)
out << puts(4)
out << print(5)
out << p(6)
out << require(7)
out << loop(8)
out << sleep(9)
out << exit(10)
out << gets(11)
out << sprintf(12)
out << format(13)
out << rand(14)
STDOUT.write(out.join("\n") + "\n")

# a name the class does not define still reaches the top-level def from inside
# the class
class K
  def viaTop = caller(21)
end

STDOUT.write(K.new.viaTop)
STDOUT.write("\n")
