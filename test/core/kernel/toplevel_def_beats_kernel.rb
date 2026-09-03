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

class K
  def viaTop = caller(21)
end

STDOUT.write(K.new.viaTop)
STDOUT.write("\n")
__END__
raise:1
system:2
caller:3
puts:4
print:5
p:6
require:7
loop:8
sleep:9
exit:10
gets:11
sprintf:12
format:13
rand:14
caller:21
