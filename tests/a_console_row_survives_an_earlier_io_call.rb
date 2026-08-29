# The `io/console` rows are gated on the require, and touching an IO BEFORE
# that require must not close the gate for the rest of the program.
#
# It did. The dispatch registry flattens a class's whole row set into a
# fill-once map on the first dynamic send, so `r.fileno` on line 1 froze the
# pre-require IO table -- and every `echo?`/`raw`/`getch` after the require
# raised NoMethodError, at every call site, for the rest of the run.
# `respond_to?` walked the tables live and answered true the whole time, so
# the two disagreed.

r, w = IO.pipe
r.fileno # the send that used to freeze the table

require "io/console"

p r.respond_to?(:echo?)
p r.respond_to?(:raw)
p IO.respond_to?(:console)

# A pipe is not a terminal, so the ANSWER is a syscall error -- which is the
# point: the row is reached at all.
begin
  r.echo?
rescue SystemCallError => e
  p e.class
end

begin
  r.raw { 1 }
rescue SystemCallError => e
  p e.class
end
