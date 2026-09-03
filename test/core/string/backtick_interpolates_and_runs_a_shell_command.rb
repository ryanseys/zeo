# Interpolation shares the normal dstr path; a `;` forces the `/bin/sh -c`
# path (shell metacharacter), while the bare word execs directly.

name = "world"
print `echo hi #{name}`
print `echo a; echo b`
puts $?.exited?
__END__
hi world
a
b
true
